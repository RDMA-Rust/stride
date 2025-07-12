use crate::cli::plan::Plan;
use crate::connection::exchange::ConnectionSetupResult;
use crate::connection::session::ConnectionSession;
use crate::connection::{ConnectionParams, EndpointRole};
use crate::context::device::open_device_context;
use crate::memory::{AlignedConfig, HugepageConfig, MemoryAllocator, MemoryType};
use crate::runners::worker::{Worker, WorkerContext, WorkerResult};
use crate::transport::flow_context;
use crate::utils::display::{
    BandwidthResult, DisplayOutput, LatencyResult, QueuePairDetail, TestConfiguration, TestType,
};
use crate::utils::random;
use anyhow::Result;
use byte_unit::Byte;
use quanta::{Clock, Instant, IntoNanoseconds};
use sideway::ibverbs::address::Gid;
use sideway::ibverbs::completion::WorkCompletionStatus;
use sideway::ibverbs::device::DeviceInfo;
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::{
    PostSendGuard, QueuePair, SetScatterGatherEntry, WorkRequestFlags,
};
use sideway::ibverbs::AccessFlags;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// New Worker-based test runner that replaces the old monolithic approach
pub struct PlanTestRunner {
    plan: Plan,
}

impl PlanTestRunner {
    pub fn new(plan: Plan) -> Self {
        Self { plan }
    }

    /// Setup flow control resources for credit-based flow control
    fn setup_flow_control<'a, 'b>(
        &self,
        pd: &'b ProtectionDomain<'a>,
        rx_depth: u32,
    ) -> Result<Option<flow_context::FlowControlContext<'a>>>
    where
        'b: 'a,
    {
        // Only setup flow control if it's enabled
        if !self.plan.uses_flow_control() {
            return Ok(None);
        }

        info!("Setting up flow control with rx_depth={}", rx_depth);

        // Create a flow control context that manages both sender and receiver
        let mut fc_context = flow_context::FlowControlContext::new(rx_depth)?;

        // Register memory regions for flow control
        fc_context.register_mr(pd)?;

        Ok(Some(fc_context))
    }

    /// Setup connection using the existing session logic but with Worker interface
    fn setup_connection<'a>(
        &self,
        worker: &Worker<'a>,
        worker_context: &mut WorkerContext<'a>,
        total_buffer_size: usize,
    ) -> Result<(ConnectionSetupResult, ConnectionSession<'a>)> {
        let gid_index = self.plan.base().gid_index.unwrap_or(0);
        let server_mode = self.plan.base().server;

        // Create connection parameters
        let mut conn_params = ConnectionParams {
            role: if server_mode {
                info!("Running in server mode");
                EndpointRole::Server
            } else {
                info!("Running in client mode");
                EndpointRole::Client
            },
            ..Default::default()
        };

        // Adjust timeout based on QP timeout parameter
        let timeout_factor = self.plan.base().timeout;
        conn_params.timeout = Duration::from_micros(4 * (1u64 << timeout_factor));

        let mut session = ConnectionSession::new(
            "tcp",
            worker.device.clone(),
            worker.pd.clone(),
            conn_params,
            gid_index,
            self.plan.mtu(),
        )?;

        // Initialize the connection session
        let _ = session.initialize();

        // Establish connection
        let address = &self.plan.base().addr;
        session.establish_connection(address)?;

        let gid_entry = worker.device.query_gid_ex(1, gid_index as u32)?;
        let gid_type = gid_entry.gid_type();
        let local_gid = gid_entry.gid();
        let mut remote_gid = Gid::default();

        // Setup each queue pair
        debug!(
            "Setting up {} queue pairs",
            worker_context.queue_pair_count()
        );
        let mut actual_mtu = 4096; // default fallback
        let mut qp_connections = Vec::new();

        for (i, qp) in worker_context.queue_pairs.iter_mut().enumerate() {
            // Create local destination info
            let local_psn = random::generate_psn();
            let local_data = crate::connection::exchange::DestinationInfo {
                qp_number: qp.qp_number(),
                psn: local_psn,
                lid: 0, // Will be filled by session
                gid: local_gid,
                gid_index: gid_index,
                gid_type: gid_type,
                mtu: self.plan.mtu(),
            };

            // Setup the QP
            let remote_data = session.setup_queue_pair(qp)?;
            remote_gid = remote_data.gid;

            // Store QP connection details for display
            qp_connections.push(crate::connection::exchange::QueuePairConnection {
                local_qpn: local_data.qp_number,
                local_psn: local_data.psn,
                remote_qpn: remote_data.qp_number,
                remote_psn: remote_data.psn,
            });

            // Get negotiated MTU after first QP setup
            if i == 0 {
                if let Some(negotiated) = session.negotiated_mtu() {
                    actual_mtu = crate::utils::mtu::mtu_to_value(negotiated);
                }
            }

            info!(
                local_qpn = local_data.qp_number,
                remote_qpn = remote_data.qp_number,
                remote_psn = format!("0x{:x}", remote_data.psn),
                "QP #{i} setup complete.",
            );
        }

        // Exchange memory regions after QP setup
        debug!("Exchanging memory region information...");
        let remote_mr = if let Some(mr) = worker.primary_memory_region() {
            // Legacy dedicated memory path
            session.exchange_memory_regions(mr.get_ptr() as u64, mr.rkey(), mr.region_len())?
        } else {
            // Shared memory path
            session.exchange_memory_regions(
                worker.base_addr as u64,
                worker.lkey(),
                total_buffer_size,
            )?
        };

        session.synchronize_qps()?;

        Ok((
            ConnectionSetupResult {
                remote_mr,
                gid_type,
                local_gid,
                remote_gid,
                actual_mtu,
                qp_details: qp_connections,
            },
            session,
        ))
    }

    /// Execute server-side receive-only mode (for unidirectional traffic)
    fn execute_server_receive_only(
        &self,
        worker_context: &mut WorkerContext,
    ) -> Result<WorkerResult> {
        // Server just waits - the actual traffic handling is done by the RDMA hardware
        // Return a dummy result since server doesn't generate traffic in unidirectional mode
        Ok(WorkerResult {
            thread_id: 0,
            completed_requests: 0,
            total_requests: worker_context.total_requests,
            execution_time_ns: 0,
            latency_samples: Vec::new(),
            error_count: 0,
        })
    }

    /// Execute RDMA operations for a single worker
    fn execute_worker<'a>(
        &self,
        worker: &Worker<'a>,
        worker_context: &mut WorkerContext<'a>,
        msg_size: u32,
        remote_mr: &crate::connection::exchange::MemoryRegionInfo,
        histogram: &mut hdrhistogram::Histogram<u64>,
    ) -> Result<WorkerResult> {
        let clock = Clock::new();
        let mut result = WorkerResult::new(worker.thread_id, worker_context.total_requests);

        let is_latency = self.plan.is_latency();
        let tx_depth = worker.tx_depth;

        let mut min_latency_ns: u64 = u64::MAX;
        let mut max_latency_ns: u64 = 0;

        let cq = worker_context.completion_queue().clone();

        let start_time = clock.now();

        // Use separate callbacks for latency and bandwidth tests
        if is_latency && tx_depth == 1 {
            self.execute_latency_test(worker, worker_context, msg_size, remote_mr, histogram, &clock, &cq, start_time, &mut min_latency_ns, &mut max_latency_ns)?;
        } else {
            self.execute_bandwidth_test(worker, worker_context, msg_size, remote_mr, &cq)?;
        }

        let end_time = clock.now();
        result.execution_time_ns = end_time.duration_since(start_time).into_nanos();
        result.completed_requests = worker_context.completed_requests;

        // Collect latency samples if this was a latency test
        if is_latency {
            for value in histogram.iter_recorded() {
                result.latency_samples.push(value.value_iterated_to());
            }
        }

        Ok(result)
    }

    /// Latency test callback - post one operation, wait for completion, repeat
    fn execute_latency_test<'a>(
        &self,
        worker: &Worker<'a>,
        worker_context: &mut WorkerContext<'a>,
        msg_size: u32,
        remote_mr: &crate::connection::exchange::MemoryRegionInfo,
        histogram: &mut hdrhistogram::Histogram<u64>,
        clock: &Clock,
        cq: &Rc<RefCell<sideway::ibverbs::completion::ExtendedCompletionQueue>>,
        start_time: Instant,
        min_latency_ns: &mut u64,
        max_latency_ns: &mut u64,
    ) -> Result<()> {
        let tx_depth = worker.tx_depth;

        while !worker_context.is_complete() {
            if worker_context.can_post_request(tx_depth) {
                let operation_start_time = clock.now(); // Precise timing for single operation

                // Post single operation across QPs (for tx_depth=1, this is typically 1 op)
                let post_list = 1;
                self.post_operations(worker, worker_context, post_list, msg_size, remote_mr)?;

                // Immediately wait for this operation's completion
                self.wait_for_completion(
                    cq,
                    operation_start_time,
                    histogram,
                    min_latency_ns,
                    max_latency_ns,
                    worker_context,
                    clock,
                )?;
            } else {
                // No more operations to post, just wait for remaining completions
                if worker_context.inflight_requests > 0 {
                    self.wait_for_completion(
                        cq,
                        start_time, // Use start_time for remaining ops
                        histogram,
                        min_latency_ns,
                        max_latency_ns,
                        worker_context,
                        clock,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Bandwidth test callback - batch posting and polling
    fn execute_bandwidth_test<'a>(
        &self,
        worker: &Worker<'a>,
        worker_context: &mut WorkerContext<'a>,
        msg_size: u32,
        remote_mr: &crate::connection::exchange::MemoryRegionInfo,
        cq: &Rc<RefCell<sideway::ibverbs::completion::ExtendedCompletionQueue>>,
    ) -> Result<()> {
        let tx_depth = worker.tx_depth;

        while !worker_context.is_complete() {
            // Post operations if we can - distribute across multiple QPs
            while worker_context.can_post_request(tx_depth) {
                let post_list = self.plan.base().post_list.min(
                    worker_context.total_requests
                        - worker_context.completed_requests
                        - worker_context.inflight_requests,
                ) as usize;

                if post_list == 0 {
                    break;
                }

                // Use helper method to post operations
                self.post_operations(worker, worker_context, post_list, msg_size, remote_mr)?;
            }

            // For bandwidth tests, poll completions in batches
            self.poll_completions_once(cq, worker_context)?;
        }
        Ok(())
    }

    /// Wait for a single completion (latency mode)
    #[inline]
    fn wait_for_completion(
        &self,
        cq: &Rc<RefCell<sideway::ibverbs::completion::ExtendedCompletionQueue>>,
        start_time: Instant,
        histogram: &mut hdrhistogram::Histogram<u64>,
        min_latency_ns: &mut u64,
        max_latency_ns: &mut u64,
        worker_context: &mut WorkerContext,
        clock: &Clock,
    ) -> Result<()> {
        let timeout_sec = 120;
        let timeout = Duration::from_secs(timeout_sec);
        let deadline = std::time::Instant::now() + timeout;

        loop {
            // Check for timeout
            if std::time::Instant::now() > deadline {
                return Err(anyhow::anyhow!(
                    "Timeout waiting for completion after {timeout_sec} seconds. Inflight: {}, Completed: {}/{}",
                    worker_context.inflight_requests,
                    worker_context.completed_requests,
                    worker_context.total_requests
                ));
            }

            // Proper CQ polling pattern following main.rs example
            match cq.borrow_mut().start_poll() {
                Ok(mut poller) => {
                    while let Some(wc) = poller.next() {
                        if wc.status() != WorkCompletionStatus::Success as u32 {
                            return Err(anyhow::anyhow!(
                                "Failed status {:?} ({}) for iteration {}",
                                Into::<WorkCompletionStatus>::into(wc.status()),
                                wc.status(),
                                wc.wr_id() & 0xFFFFFFFF
                            ));
                        }

                        // Measure completion time for latency
                        let completion_time = clock.now();
                        let latency_ns = completion_time.duration_since(start_time).into_nanos();

                        // Record latency
                        histogram.record(latency_ns)?;
                        *min_latency_ns = (*min_latency_ns).min(latency_ns);
                        *max_latency_ns = (*max_latency_ns).max(latency_ns);

                        // For latency tests, we typically use QP 0, so record completion on QP 0
                        worker_context.record_qp_requests_completed(0, 1);
                        return Ok(());
                    }
                }
                Err(_) => {
                    continue;
                }
            }
        }
    }

    /// Poll completions once and return whether any were found (bandwidth mode)
    #[inline(always)]
    fn poll_completions_once(
        &self,
        cq: &Rc<RefCell<sideway::ibverbs::completion::ExtendedCompletionQueue>>,
        worker_context: &mut WorkerContext,
    ) -> Result<bool> {
        let cqe_poll_limit = self.plan.poll_batch(); // Respect user-configured batch size

        // Proper CQ polling pattern - poll up to cqe_poll_limit completions to prevent bubbles
        match cq.borrow_mut().start_poll() {
            Ok(mut poller) => {
                // PERFORMANCE CRITICAL: Batch completion counting with per-QP tracking
                let mut completed_count = 0u32;
                let mut qp_completion_counts = vec![0u32; worker_context.queue_pair_count()];

                // Poll up to cqe_poll_limit completions to maintain posting/polling balance
                // This prevents pipeline bubbles when completions arrive very fast
                while completed_count < cqe_poll_limit {
                    if let Some(wc) = poller.next() {
                        // Hot path: Use const comparison for maximum performance
                        if wc.status() != (WorkCompletionStatus::Success as u32) {
                            return Err(anyhow::anyhow!(
                                "Failed status {:?} ({}) for iteration {}",
                                Into::<WorkCompletionStatus>::into(wc.status()),
                                wc.status(),
                                wc.wr_id() & 0xFFFFFFFF
                            ));
                        }

                        // Extract QP index from wr_id (we can derive it from completion queue)
                        // For now, distribute completions evenly across QPs
                        // TODO: Extract actual QP index from wr_id if needed
                        let qp_idx = (completed_count as usize) % worker_context.queue_pair_count();
                        qp_completion_counts[qp_idx] += 1;
                        completed_count += 1;
                    } else {
                        // No more completions available right now
                        break;
                    }
                }

                // Batch update: update per-QP completion counts (perftest-style ccnt tracking)
                if completed_count > 0 {
                    for (qp_idx, qp_completions) in qp_completion_counts.iter().enumerate() {
                        if *qp_completions > 0 {
                            worker_context.record_qp_requests_completed(qp_idx, *qp_completions);
                        }
                    }
                    Ok(true) // Found completions
                } else {
                    Ok(false) // No completions in this poll
                }
            }
            Err(_) => {
                // No completions available
                Ok(false)
            }
        }
    }

    /// Helper method to post operations to QPs
    fn post_operations<'a>(
        &self,
        worker: &Worker<'a>,
        worker_context: &mut WorkerContext<'a>,
        post_list: usize,
        msg_size: u32,
        remote_mr: &crate::connection::exchange::MemoryRegionInfo,
    ) -> Result<()> {
        let is_write = self.plan.needs_remote_addr();
        let tx_depth = worker.tx_depth;
        let thread_id_shifted = (worker.thread_id as u64) << 32;

        // Optimize: Replace expensive modulo with bitwise AND (tx_depth must be power of 2)
        debug_assert!(
            tx_depth.is_power_of_two(),
            "tx_depth must be power of 2 for bitwise optimization"
        );
        let tx_depth_mask = tx_depth - 1;

        // Create buffers for state to avoid borrow conflicts
        let completed = worker_context.completed_requests;
        let inflight = worker_context.inflight_requests;
        let qp_count = worker_context.queue_pair_count();

        // perftest approach: each QP posts post_list operations (not split across QPs)
        // With per-QP tx_depth control like perftest's scnt/ccnt pattern
        for qp_idx in 0..qp_count {
            // perftest-style per-QP flow control check
            if !worker_context.can_qp_post_request(qp_idx, tx_depth) {
                continue; // Skip this QP if it's at tx_depth limit
            }

            // Calculate how many operations this QP can actually post
            let qp_inflight = worker_context.qp_send_counts[qp_idx] - worker_context.qp_completion_counts[qp_idx];
            let qp_available_slots = tx_depth - qp_inflight;
            let actual_post_list = post_list.min(qp_available_slots as usize);

            if actual_post_list == 0 {
                continue; // No slots available for this QP
            }

            // Pre-calculate base values for this QP's operations
            let qp_operation_base = worker_context.qp_send_counts[qp_idx];

            // Pre-calculate all addresses before mutable borrow to avoid borrow conflicts
            let tx_depth_usize = tx_depth as usize;
            let qp_base_index = qp_idx * tx_depth_usize;

            // Extract raw pointers to address arrays before mutable borrow
            let local_addrs_ptr = worker_context.qp_buffer_addrs.as_ptr();
            let remote_addrs_ptr = worker_context.qp_remote_addrs.as_ptr();

            // Get QP (unchecked for performance)
            // SAFETY: qp_idx < qp_count, which is the number of QPs we created
            let qp = unsafe { worker_context.get_queue_pair_mut_unchecked(qp_idx) };

            // Create post guard for this QP
            let mut guard = qp.start_post_send();

            // Each QP posts actual_post_list operations (perftest style with flow control)
            for i in 0..actual_post_list {
                // Global index for wr_id tracking (includes QP information)
                let global_op_index = qp_operation_base + i as u32;
                let wr_id = thread_id_shifted | (global_op_index as u64);

                // PERFORMANCE CRITICAL: Direct flat index calculation for maximum performance
                let operation_within_qp = (qp_operation_base + i as u32) & tx_depth_mask;
                let flat_index = qp_base_index + operation_within_qp as usize;

                // Get addresses directly from flattened arrays using raw pointers
                let local_addr = unsafe { *local_addrs_ptr.add(flat_index) };

                // For WRITE operations, use pre-calculated remote addresses
                let send_handle = if is_write {
                    let remote_addr = unsafe { *remote_addrs_ptr.add(flat_index) };
                    guard
                        .construct_wr(wr_id, WorkRequestFlags::Signaled)
                        .setup_write(remote_mr.rkey, remote_addr)
                } else {
                    guard
                        .construct_wr(wr_id, WorkRequestFlags::Signaled)
                        .setup_send()
                };

                // Setup scatter-gather entry
                unsafe {
                    send_handle.setup_sge(worker.lkey(), local_addr, msg_size);
                }
            }

            // Post all operations for this QP
            guard.post()?;

            // Update per-QP counters (perftest-style scnt tracking)
            worker_context.record_qp_requests_posted(qp_idx, actual_post_list as u32);
        }
        Ok(())
    }

    /// Main execution function using Worker architecture
    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "Starting {} with Worker architecture",
            self.plan.test_name()
        );

        // Get parameters from plan
        let device_name = self.plan.base().dev.as_deref();
        let iterations = self.plan.base().iters;
        let msg_sizes = &self.plan.base().msg_sizes;
        let tx_depth = self.plan.base().tx_depth;
        let qp_count = self.plan.base().threads;

        info!("Will test {} message sizes", msg_sizes.len());

        let ctx = Arc::new(open_device_context(device_name)?);
        let pd = Arc::new(ctx.alloc_pd()?);

        // Determine test type
        let test_type = match (self.plan.operation(), self.plan.is_latency()) {
            (crate::cli::plan::Operation::Send, true) => TestType::SendLatency,
            (crate::cli::plan::Operation::Send, false) => TestType::SendBandwidth,
            (crate::cli::plan::Operation::Write, true) => TestType::WriteLatency,
            (crate::cli::plan::Operation::Write, false) => TestType::WriteBandwidth,
            (crate::cli::plan::Operation::Read, true) => TestType::ReadLatency,
            (crate::cli::plan::Operation::Read, false) => TestType::ReadBandwidth,
            _ => TestType::SendBandwidth, // fallback
        };

        let is_latency = test_type.is_latency();
        let max_msg_size = *msg_sizes.iter().max().unwrap_or(&65536);

        // Create workers using perftest-style shared memory allocation
        // Following perftest pattern: shared buffer across QPs with cache-aligned cycling

        // Calculate buffer size following perftest BUFF_SIZE and INC patterns exactly
        const CYCLE_BUFFER_SIZE: usize = 4096; // perftest cycle_buffer
        const CACHE_LINE_SIZE: usize = 64; // Standard cache line size

        // perftest BUFF_SIZE macro: ensure minimum cycle buffer size for small messages
        let buff_size = if max_msg_size < CYCLE_BUFFER_SIZE as u32 {
            CYCLE_BUFFER_SIZE
        } else {
            max_msg_size as usize
        };

        // perftest INC macro: cache-aligned increment size calculation
        let increment_size = if buff_size > CACHE_LINE_SIZE {
            // Round up to cache line boundary (like perftest ROUND_UP)
            (buff_size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1)
        } else {
            CACHE_LINE_SIZE
        };

        // perftest-style buffer calculation: increment * 2 (send/recv) * qp_factor
        // tx_depth is handled by cycling through addresses, not buffer size multiplication
        let total_buffer_size = increment_size * 2 * qp_count;

        info!(
            "Memory allocation: msg_size={} -> buff_size={}, increment={}, total_buffer={}KB",
            max_msg_size,
            buff_size,
            increment_size,
            total_buffer_size / 1024
        );

        // Allocate single shared memory region for all QPs (perftest approach)
        let memory_type = if self.plan.base().hugepages {
            MemoryType::Hugepages(HugepageConfig::new(total_buffer_size))
        } else {
            MemoryType::Aligned(AlignedConfig::new(total_buffer_size, CACHE_LINE_SIZE))
        };

        let shared_memory = MemoryAllocator::allocate(memory_type)?;

        // Create single shared memory region
        let shared_mr = unsafe {
            pd.reg_mr(
                shared_memory.get_handle(),
                shared_memory.size(),
                AccessFlags::LocalWrite | AccessFlags::RemoteWrite | AccessFlags::RelaxedOrdering,
            )?
        };

        // Create workers sharing the same memory region (following perftest pattern exactly)
        let mut workers = Vec::with_capacity(qp_count);
        for thread_id in 0..qp_count {
            // perftest QP offset calculation: each QP gets increment * 2 space
            // This ensures proper spacing for both send and receive buffers per QP
            let qp_offset = thread_id * increment_size * 2;

            let worker = Worker::new_with_shared_memory(
                ctx.clone(),
                pd.clone(),
                shared_mr.lkey(),                      // Just pass the lkey
                shared_memory.get_handle() as *mut u8, // Base pointer for address calculation
                increment_size,                        // perftest INC value for this worker
                qp_offset,                            // perftest-style QP offset calculation
                self.plan.clone(),
                thread_id,
                tx_depth,
                Some(512), // rx_depth
            );

            workers.push(worker);
        }

        // Keep shared memory alive
        let _shared_memory_handle = shared_memory;

        // For now, we'll run single-threaded with the first worker
        // TODO: Implement multi-threaded execution later
        let worker = &workers[0];
        let mut worker_context = WorkerContext::new(worker, &self.plan, iterations)?;

        // Add queue pairs to the worker context
        for _ in 0..qp_count {
            worker_context.add_queue_pair(worker)?;
        }

        // Setup connection using the worker
        let (conn_result, mut session) =
            self.setup_connection(worker, &mut worker_context, total_buffer_size)?;

        // Update pre-calculated remote addresses now that we have remote MR info
        worker_context.update_remote_addresses(conn_result.remote_mr.addr);

        // For SEND operations, post receive buffers on both client and server
        if self.plan.needs_receive_buffers() {
            if let Some(rx_depth) = self.plan.rx_depth() {
                let initial_rx_buffers = rx_depth; // Start with fewer buffers
                info!("Posting {} receive buffers for SEND operations (max_msg_size={})", initial_rx_buffers, max_msg_size);
                worker_context.post_receive_buffers(worker, initial_rx_buffers, max_msg_size)?;
                info!("Successfully posted receive buffers");
            }
        }

        // Create display output for results
        let config = TestConfiguration {
            device: ctx.name(),
            transport: ctx.transport_type().to_string(),
            qp_count: qp_count as u32,
            connection_type: "RC".to_string(),
            mtu: conn_result.actual_mtu,
            gid_type: format!("{:?}", conn_result.gid_type),
            rx_depth: self.plan.rx_depth().unwrap_or(512),
            tx_depth,
            post_list: self.plan.base().post_list,
            test_type,
            uses_immediate_data: self.plan.uses_immediate_data(),
        };

        let qp_details: Vec<QueuePairDetail> = conn_result
            .qp_details
            .iter()
            .enumerate()
            .map(|(i, qp_conn)| QueuePairDetail {
                qp_index: i as u32,
                local_qpn: qp_conn.local_qpn,
                local_psn: qp_conn.local_psn,
                remote_qpn: qp_conn.remote_qpn,
                remote_psn: qp_conn.remote_psn,
            })
            .collect();

        let gid_info = vec![conn_result.local_gid, conn_result.remote_gid];
        let mut display = DisplayOutput::new(
            config,
            qp_details,
            gid_info,
            self.plan.base().output.clone(),
        );

        // Display headers and configuration only for clients and bidirectional servers
        let is_server = self.plan.base().server;
        let is_bidirectional = self.plan.is_bidirectional();

        let mut table_formatter = if is_server && !is_bidirectional {
            // Unidirectional server mode: display headers but initialize table later when receiving results
            info!("Running in server mode (unidirectional) - waiting for client connections...");
            let header_width = if is_latency {
                crate::utils::display::DEFAULT_LAT_HEADER_WIDTH
            } else {
                crate::utils::display::DEFAULT_HEADER_WIDTH
            };
            display.display_headers_only(header_width);

            // Initialize streaming table for received results
            if is_latency {
                Some(display.init_latency_streaming_table(header_width)?)
            } else {
                Some(display.init_bandwidth_streaming_table(header_width)?)
            }
        } else {
            // Client mode or bidirectional mode: display headers and initialize table
            let header_width = if is_latency {
                crate::utils::display::DEFAULT_LAT_HEADER_WIDTH
            } else {
                crate::utils::display::DEFAULT_HEADER_WIDTH
            };
            display.display_headers_only(header_width);

            // Initialize streaming table for real-time results
            if is_latency {
                Some(display.init_latency_streaming_table(header_width)?)
            } else {
                Some(display.init_bandwidth_streaming_table(header_width)?)
            }
        };

        // Execute tests for each message size
        for &msg_size in msg_sizes {
            info!(
                message_size = msg_size,
                "Running test with Worker architecture"
            );

            let mut histogram = hdrhistogram::Histogram::<u64>::new(3).unwrap();

            // Execute the test based on mode
            let result = if is_bidirectional || !is_server {
                // Execute traffic generation (both directions in bidir mode, or client-only in unidir mode)
                self.execute_worker(
                    worker,
                    &mut worker_context,
                    msg_size,
                    &conn_result.remote_mr,
                    &mut histogram,
                )?
            } else {
                // Server-only mode: just wait and receive (no traffic generation)
                self.execute_server_receive_only(&mut worker_context)?
            };

            // Process results based on mode and role
            if is_server && !is_bidirectional {
                // Unidirectional server mode: receive results from client and display them
                info!(
                    "Waiting to receive results for message size {} from client...",
                    msg_size
                );
                let received_results = session
                    .receive_results()
                    .map_err(|e| anyhow::anyhow!("Failed to receive results from client: {}", e))?;

                // Display the received results
                if received_results.latency_result.is_some() {
                    let lat_results = received_results.latency_result.unwrap();
                    info!(
                        "Received latency results: {:.3} μs avg",
                        lat_results.avg_latency
                    );

                    if let Some(ref mut formatter) = table_formatter {
                        formatter.print_row_data(&lat_results)?;
                    }
                    display.add_latency_result(lat_results);
                } else if received_results.bandwidth_result.is_some() {
                    let bw_results = received_results.bandwidth_result.unwrap();
                    info!(
                        "Received bandwidth results: {:.3} Gbps",
                        bw_results.bandwidth
                    );

                    if let Some(ref mut formatter) = table_formatter {
                        formatter.print_row_data(&bw_results)?;
                    }
                    display.add_bandwidth_result(bw_results);
                }
            } else if is_latency {
                let lat_results = self.calculate_latency_results(msg_size, result, &histogram);
                info!(
                    "Latency test completed: {:.3} μs avg",
                    lat_results.avg_latency
                );

                // Print result immediately for streaming output
                if let Some(ref mut formatter) = table_formatter {
                    formatter.print_row_data(&lat_results)?;
                }
                display.add_latency_result(lat_results.clone());

                // Send results to server in unidirectional mode
                if !is_server && !is_bidirectional {
                    let test_results = crate::connection::exchange::TestResults {
                        test_type: crate::connection::exchange::TestType::Latency,
                        size: msg_size,
                        iterations: lat_results.iterations,
                        time: "0.00".to_string(),
                        bandwidth_result: None,
                        latency_result: Some(lat_results),
                    };
                    session.send_results(&test_results).map_err(|e| {
                        anyhow::anyhow!("Failed to send latency results to server: {}", e)
                    })?;
                }
            } else {
                // Handle bandwidth results with bidirectional support
                let mut bw_results = self.calculate_bandwidth_results(msg_size, result);

                // For bidirectional mode, double the bandwidth (traffic in both directions)
                if is_bidirectional {
                    bw_results.bandwidth *= 2.0;
                    bw_results.msg_rate *= 2.0;
                    info!(
                        "Bidirectional bandwidth test completed: {:.3} Gbps (aggregated)",
                        bw_results.bandwidth
                    );
                } else {
                    info!("Bandwidth test completed: {:.3} Gbps", bw_results.bandwidth);
                }

                // Print result immediately for streaming output
                if let Some(ref mut formatter) = table_formatter {
                    formatter.print_row_data(&bw_results)?;
                }
                display.add_bandwidth_result(bw_results.clone());

                // Send results to server in unidirectional mode
                if !is_server && !is_bidirectional {
                    let test_results = crate::connection::exchange::TestResults {
                        test_type: crate::connection::exchange::TestType::Bandwidth,
                        size: msg_size,
                        iterations: bw_results.iterations,
                        time: bw_results.time.clone(),
                        bandwidth_result: Some(bw_results),
                        latency_result: None,
                    };
                    session.send_results(&test_results).map_err(|e| {
                        anyhow::anyhow!("Failed to send bandwidth results to server: {}", e)
                    })?;
                }
            }

            // Reset worker context for next iteration
            worker_context.completed_requests = 0;
            worker_context.inflight_requests = 0;
        }

        // Print table footer if we have a formatter
        if let Some(ref formatter) = table_formatter {
            formatter.print_bottom_separator()?;
        }

        // CRITICAL: Synchronize both sides before cleanup to prevent premature connection close
        // This is essential for bidirectional mode where both sides run tests simultaneously
        let is_server = self.plan.base().server;
        let is_bidirectional = self.plan.is_bidirectional();

        if is_bidirectional {
            info!("Synchronizing with remote peer before cleanup (bidirectional mode)...");
            session.synchronize_qps().map_err(|e| {
                anyhow::anyhow!(
                    "Failed to synchronize with remote peer before cleanup: {}",
                    e
                )
            })?;
            info!("Synchronization complete. Closing session...");
        } else if !is_server {
            // Unidirectional client mode: just close normally
            info!("Closing session (unidirectional client mode)...");
        } else {
            // Unidirectional server mode: just close normally
            info!("Closing session (unidirectional server mode)...");
        }

        session.close()?;
        Ok(())
    }

    fn calculate_latency_results(
        &self,
        msg_size: u32,
        result: WorkerResult,
        histogram: &hdrhistogram::Histogram<u64>,
    ) -> LatencyResult {
        let min_latency_ns = result.latency_samples.iter().min().unwrap_or(&0);
        let max_latency_ns = result.latency_samples.iter().max().unwrap_or(&0);

        LatencyResult {
            size: msg_size,
            iterations: result.completed_requests,
            min_latency: (*min_latency_ns as f64) / 1000.0,
            max_latency: (*max_latency_ns as f64) / 1000.0,
            typical_latency: histogram.value_at_quantile(0.5) as f64 / 1000.0,
            avg_latency: histogram.mean() / 1000.0,
            stdev_latency: histogram.stdev() / 1000.0,
            p99_latency: histogram.value_at_quantile(0.99) as f64 / 1000.0,
            p999_latency: histogram.value_at_quantile(0.999) as f64 / 1000.0,
        }
    }

    fn calculate_bandwidth_results(&self, msg_size: u32, result: WorkerResult) -> BandwidthResult {
        let total_bytes = msg_size as u64 * result.completed_requests as u64;
        let time_seconds = result.execution_time_ns as f64 / 1_000_000_000.0;
        let bytes_per_second = total_bytes as f64 / time_seconds;

        BandwidthResult {
            size: msg_size,
            iterations: result.completed_requests,
            bandwidth: Byte::from_f64(bytes_per_second)
                .unwrap()
                .get_adjusted_unit(byte_unit::Unit::Gbit)
                .get_value(),
            msg_rate: (result.completed_requests as f64) / time_seconds / 1_000_000.0,
            time: format!("{:.2}", time_seconds),
        }
    }
}
