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
use sideway::ibverbs::queue_pair::{PostSendGuard, QueuePair, SetScatterGatherEntry, WorkRequestFlags};
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
        debug!("Setting up {} queue pairs", worker_context.queue_pair_count());
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
        let mr = worker.primary_memory_region();
        let remote_mr =
            session.exchange_memory_regions(mr.get_ptr() as u64, mr.rkey(), mr.region_len())?;

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
        let start_time = clock.now();
        let mut result = WorkerResult::new(worker.thread_id, worker_context.total_requests);

        let is_latency = self.plan.is_latency();
        let is_write = self.plan.needs_remote_addr();
        let tx_depth = worker.tx_depth;

        let mut min_latency_ns: u64 = u64::MAX;
        let mut max_latency_ns: u64 = 0;

        let cq = worker_context.completion_queue().clone();
        let mr = worker.primary_memory_region();

        while !worker_context.is_complete() {
            // Post operations if we can
            while worker_context.can_post_request(tx_depth) {
                let post_list = self.plan.base().post_list.min(
                    worker_context.total_requests - worker_context.completed_requests - worker_context.inflight_requests
                ) as usize;

                if post_list == 0 {
                    break;
                }

                // Take timestamp for latency measurements
                let _operation_start_time = if is_latency { Some(clock.now()) } else { None };

                // Create buffers for state to avoid borrow conflicts
                let completed = worker_context.completed_requests;
                let inflight = worker_context.inflight_requests;

                // Get QP in limited scope (unchecked for performance)
                // SAFETY: We always create at least 1 QP during setup, index 0 is guaranteed valid
                let qp = unsafe { worker_context.get_queue_pair_mut_unchecked(0) };

                // Create a single post guard
                let mut guard = qp.start_post_send();

                // Hot path: Calculate base values outside loop for performance
                let base_ptr = mr.get_ptr() as u64;
                let msg_size_u64 = msg_size as u64;
                let thread_id_shifted = (worker.thread_id as u64) << 32;
                
                // Post up to post_list operations at once
                for i in 0..post_list {
                    // Calculate buffer offset with unchecked arithmetic (safe: tx_depth bounds buffer allocation)
                    let iter_index = (completed + inflight + i as u32) % tx_depth;
                    let iter_offset = (iter_index as u64) * msg_size_u64;
                    let local_addr = base_ptr + iter_offset;

                    let wr_id = thread_id_shifted | ((completed + inflight + i as u32) as u64);

                    // For WRITE operations, use remote memory info
                    let send_handle = if is_write {
                        let remote_addr = remote_mr.addr + iter_offset;
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
                        send_handle.setup_sge(mr.lkey(), local_addr, msg_size);
                    }
                }

                // Post all operations at once
                guard.post()?;

                // Update counters after posting (batch update for performance)
                worker_context.record_requests_posted(post_list as u32);
            }

            // Poll for completions
            if is_latency {
                // For latency tests, wait for each completion individually
                self.wait_for_completion(
                    &cq,
                    start_time,
                    histogram,
                    &mut min_latency_ns,
                    &mut max_latency_ns,
                    worker_context,
                )?;
            } else {
                // For bandwidth tests, poll in batches
                self.poll_completions(&cq, worker_context)?;
            }
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
    ) -> Result<()> {
        loop {
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
                        let completion_time = quanta::Clock::new().now();
                        let latency_ns = completion_time.duration_since(start_time).into_nanos();

                        // Record latency
                        histogram.record(latency_ns)?;
                        *min_latency_ns = (*min_latency_ns).min(latency_ns);
                        *max_latency_ns = (*max_latency_ns).max(latency_ns);

                        worker_context.record_request_completed();
                        return Ok(());
                    }
                }
                Err(_) => continue,
            }
        }
    }

    /// Poll completions aggressively (bandwidth mode)
    #[inline(always)]
    fn poll_completions(
        &self,
        cq: &Rc<RefCell<sideway::ibverbs::completion::ExtendedCompletionQueue>>,
        worker_context: &mut WorkerContext,
    ) -> Result<()> {
        // Proper CQ polling pattern - poll ALL available completions for maximum throughput
        match cq.borrow_mut().start_poll() {
            Ok(mut poller) => {
                // Poll ALL available completions, not just a limited batch
                // This is critical for high-throughput bandwidth tests
                while let Some(wc) = poller.next() {
                    // Hot path: Use const comparison for maximum performance
                    if wc.status() != (WorkCompletionStatus::Success as u32) {
                        return Err(anyhow::anyhow!(
                            "Failed status {:?} ({}) for iteration {}",
                            Into::<WorkCompletionStatus>::into(wc.status()),
                            wc.status(),
                            wc.wr_id() & 0xFFFFFFFF
                        ));
                    }

                    worker_context.record_request_completed();
                }
            }
            Err(_) => {
                // No completions available, continue
            }
        }
        Ok(())
    }

    /// Main execution function using Worker architecture
    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting {} with Worker architecture", self.plan.test_name());

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

        // Create workers using the new Worker interface
        let mut workers = Vec::with_capacity(qp_count);
        for thread_id in 0..qp_count {
            // Create memory for this worker
            let buffer_size = tx_depth as usize * max_msg_size as usize;
            let memory_type = if self.plan.base().hugepages {
                MemoryType::Hugepages(HugepageConfig::new(buffer_size))
            } else {
                MemoryType::Aligned(AlignedConfig::new(
                    buffer_size,
                    crate::memory::aligned::DEFAULT_CACHE_LINE_SIZE,
                ))
            };

            let memory = MemoryAllocator::allocate(memory_type)?;

            // Create memory region
            let mr = unsafe {
                pd.reg_mr(
                    memory.get_handle(),
                    memory.size(),
                    AccessFlags::LocalWrite | AccessFlags::RemoteWrite | AccessFlags::RelaxedOrdering,
                )?
            };

            let worker = Worker::new(
                ctx.clone(),
                pd.clone(),
                mr,
                memory,
                self.plan.clone(),
                thread_id,
                tx_depth,
                Some(512), // rx_depth
            );

            workers.push(worker);
        }

        // For now, we'll run single-threaded with the first worker
        // TODO: Implement multi-threaded execution later
        let worker = &workers[0];
        let mut worker_context = WorkerContext::new(worker, &self.plan, iterations)?;
        
        // Add queue pairs to the worker context
        for _ in 0..qp_count {
            worker_context.add_queue_pair(worker)?;
        }

        // Setup connection using the worker
        let (conn_result, mut session) = self.setup_connection(worker, &mut worker_context)?;

        // Create display output for results  
        let config = TestConfiguration {
            device: ctx.name(),
            transport: ctx.transport_type().to_string(),
            qp_count: qp_count as u32,
            connection_type: "RC".to_string(),
            mtu: conn_result.actual_mtu,
            gid_type: format!("{:?}", conn_result.gid_type),
            rx_depth: 512,
            tx_depth,
            post_list: self.plan.base().post_list,
            test_type,
        };

        let qp_details: Vec<QueuePairDetail> = conn_result.qp_details.iter().enumerate()
            .map(|(i, qp_conn)| QueuePairDetail {
                qp_index: i as u32,
                local_qpn: qp_conn.local_qpn,
                local_psn: qp_conn.local_psn,
                remote_qpn: qp_conn.remote_qpn,
                remote_psn: qp_conn.remote_psn,
            })
            .collect();

        let gid_info = vec![conn_result.local_gid, conn_result.remote_gid];
        let mut display = DisplayOutput::new(config, qp_details, gid_info, self.plan.base().output.clone());

        // Display headers and configuration only once before testing
        let header_width = if is_latency { 
            crate::utils::display::DEFAULT_LAT_HEADER_WIDTH 
        } else { 
            crate::utils::display::DEFAULT_HEADER_WIDTH 
        };
        display.display_headers_only(header_width);

        // Initialize streaming table for real-time results
        let mut table_formatter = if is_latency {
            Some(display.init_latency_streaming_table(header_width)?)
        } else {
            Some(display.init_bandwidth_streaming_table(header_width)?)
        };

        // Execute tests for each message size
        for &msg_size in msg_sizes {
            info!(message_size = msg_size, "Running test with Worker architecture");

            let mut histogram = hdrhistogram::Histogram::<u64>::new(3).unwrap();
            
            // Execute the test
            let result = self.execute_worker(
                worker,
                &mut worker_context,
                msg_size,
                &conn_result.remote_mr,
                &mut histogram,
            )?;

            // Process results and display immediately for streaming output
            if is_latency {
                let lat_results = self.calculate_latency_results(msg_size, result, &histogram);
                info!("Latency test completed: {:.3} μs avg", lat_results.avg_latency);
                
                // Print result immediately for streaming output
                if let Some(ref mut formatter) = table_formatter {
                    formatter.print_row_data(&lat_results)?;
                }
                display.add_latency_result(lat_results);
            } else {
                let bw_results = self.calculate_bandwidth_results(msg_size, result);
                info!("Bandwidth test completed: {:.3} Gbps", bw_results.bandwidth);
                
                // Print result immediately for streaming output
                if let Some(ref mut formatter) = table_formatter {
                    formatter.print_row_data(&bw_results)?;
                }
                display.add_bandwidth_result(bw_results);
            }

            // Reset worker context for next iteration
            worker_context.completed_requests = 0;
            worker_context.inflight_requests = 0;
        }

        // Print table footer if we have a formatter
        if let Some(ref formatter) = table_formatter {
            formatter.print_bottom_separator()?;
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