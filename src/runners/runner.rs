use crate::connection::exchange::ConnectionSetupResult;
use crate::memory::aligned::HUGE_PAGE_SIZE;
use anyhow::Result;
use byte_unit::Byte;
use quanta::Clock;
use quanta::Instant;
use quanta::IntoNanoseconds;
use sideway::ibverbs::address::Gid;
use sideway::ibverbs::completion::{
    CreateCompletionQueueWorkCompletionFlags, GenericCompletionQueue, WorkCompletionStatus,
};
use sideway::ibverbs::device::DeviceInfo;
use sideway::ibverbs::device_context::DeviceContext;
use sideway::ibverbs::memory_region::MemoryRegion;
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::{
    GenericQueuePair, PostSendGuard, QueuePair, SetScatterGatherEntry, WorkRequestFlags,
};
use sideway::ibverbs::AccessFlags;
use tracing::{debug, trace};

use std::sync::Arc;
use std::time::Duration;

use crate::cli::plan::Plan;
use crate::connection::exchange::TestResults;
use crate::connection::session::ConnectionSession;
use crate::connection::ConnectionParams;
use crate::connection::EndpointRole;
use crate::context::device::open_device_context;
use crate::memory::aligned::DEFAULT_CACHE_LINE_SIZE;
use crate::memory::{AlignedConfig, HugepageConfig, MemoryAllocator, MemoryType};
use crate::transport::flow_context;
use crate::utils::display::{
    BandwidthResult, DisplayOutput, LatencyResult, QueuePairDetail, TestConfiguration, TestType,
};
use crate::utils::random;

use tracing::info;

/// Cache line size in bytes (for address alignment)
const CACHE_LINE_SIZE: usize = 64;

/// Helper to increment address with cache line alignment
/// Similar to perftest's increase_loc_addr function
#[inline]
fn increase_addr_with_alignment(
    current_addr: u64,
    msg_size: u32,
    iteration: u32,
    base_addr: u64,
    cycle_buffer_size: u32,
) -> u64 {
    // Increment address, aligned to cache line
    let incremented = current_addr + align_to_cache_line(msg_size as usize) as u64;

    // Check if we need to cycle back to the beginning of the buffer
    if cycle_buffer_size > 0
        && ((iteration + 1) % (cycle_buffer_size / align_to_cache_line(msg_size as usize) as u32))
            == 0
    {
        base_addr
    } else {
        incremented
    }
}

/// Align size to cache line boundary
#[inline]
fn align_to_cache_line(size: usize) -> usize {
    size.div_ceil(HUGE_PAGE_SIZE) * CACHE_LINE_SIZE
}
// New implementation for Plan
pub struct PlanTestRunner {
    plan: Plan,
}

impl PlanTestRunner {
    pub fn new(plan: Plan) -> Self {
        Self { plan }
    }

    // Setup flow control resources for credit-based flow control
    fn setup_flow_control<'a, 'b>(
        &self,
        pd: &'b ProtectionDomain<'a>,
        rx_depth: u32,
    ) -> anyhow::Result<Option<flow_context::FlowControlContext<'a>>>
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

    fn setup_connection<'a>(
        &self,
        ctx: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain<'a>>,
        qps: &mut [GenericQueuePair],
        mr: &MemoryRegion,
        qp_details: &mut [QueuePairDetail],
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

        let mut session =
            ConnectionSession::new("tcp", ctx.clone(), pd.clone(), conn_params, gid_index)?;

        // Initialize the connection session
        let _ = session.initialize();

        // Establish connection
        let address = &self.plan.base().addr;
        session.establish_connection(address)?;
        let actual_mtu = 4096;

        let gid_entry = ctx.query_gid_ex(1, gid_index as u32)?;
        let gid_type = gid_entry.gid_type();
        let local_gid = gid_entry.gid();
        let mut remote_gid = Gid::default();

        // Setup each queue pair
        debug!("Setting up {} queue pairs", qps.len());
        for (i, (qp, detail)) in qps.iter_mut().zip(qp_details.iter_mut()).enumerate() {
            // Store local PSN before exchange
            let local_psn = detail.local_psn;

            // Setup the QP
            let remote_data = session.setup_queue_pair(qp)?;
            remote_gid = remote_data.gid;

            // Update QP details with actual exchanged values
            detail.local_qpn = qp.qp_number();
            detail.local_psn = local_psn;
            detail.remote_qpn = remote_data.qp_number;
            detail.remote_psn = remote_data.psn;

            info!(
                local_qpn = detail.local_qpn,
                remote_qpn = detail.remote_qpn,
                remote_psn = format!("0x{:x}", detail.remote_psn),
                "QP #{i} setup complete.",
            );
        }

        // Exchange memory regions after QP setup
        debug!("Exchanging memory region information...");
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
            },
            session,
        ))
    }

    #[inline]
    fn wait_for_completion(
        &self,
        cq: &GenericCompletionQueue,
        start_time: Instant,
        histogram: &mut hdrhistogram::Histogram<u64>,
        min_latency_ns: &mut u64,
        max_latency_ns: &mut u64,
        inflight_per_qp: &mut [u32],
        clock: &Clock,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match cq.start_poll() {
                Ok(mut poller) => {
                    for wc in poller.by_ref() {
                        let wc_qp_idx = (wc.wr_id() >> 32) as usize;

                        if wc.status() != WorkCompletionStatus::Success as u32 {
                            return Err(format!(
                                "QP #{}: Failed status {:?} ({}) for iteration {}",
                                wc_qp_idx,
                                Into::<WorkCompletionStatus>::into(wc.status()),
                                wc.status(),
                                wc.wr_id() & 0xFFFFFFFF
                            )
                            .into());
                        }

                        if wc_qp_idx == 0 {
                            // Measure completion time
                            let completion_time = clock.now();

                            // Calculate latency
                            let latency_ns =
                                completion_time.duration_since(start_time).into_nanos();

                            // Record latency
                            histogram.record(latency_ns)?;
                            *min_latency_ns = (*min_latency_ns).min(latency_ns);
                            *max_latency_ns = (*max_latency_ns).max(latency_ns);

                            inflight_per_qp[wc_qp_idx] -= 1;
                            return Ok(());
                        }
                    }
                }
                Err(_) => continue,
            }
        }
    }

    #[inline]
    fn poll_completions(
        &self,
        cq: &GenericCompletionQueue,
        inflight_per_qp: &mut [u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get the CQE poll batch size from parameters
        let poll_batch_size = self.plan.poll_batch() as usize;

        // Poll completions in batches (like perftest's CTX_POLL_BATCH)
        if let Ok(poller) = cq.start_poll() {
            // Simply iterate but limit to poll_batch_size completions at once
            let mut completed = 0;

            for wc in poller {
                let qp_idx = (wc.wr_id() >> 32) as usize;

                if wc.status() != WorkCompletionStatus::Success as u32 {
                    return Err(format!(
                        "QP #{}: Failed status {:?} ({}) for iteration {}",
                        qp_idx,
                        Into::<WorkCompletionStatus>::into(wc.status()),
                        wc.status(),
                        wc.wr_id() & 0xFFFFFFFF
                    )
                    .into());
                }

                inflight_per_qp[qp_idx] -= 1;
                completed += 1;

                // Stop polling after processing poll_batch_size completions
                if completed >= poll_batch_size {
                    break;
                }
            }
        }
        Ok(())
    }

    fn calculate_latency_results(
        &self,
        msg_size: u32,
        iterations: u32,
        histogram: &hdrhistogram::Histogram<u64>,
        min_latency_ns: u64,
        max_latency_ns: u64,
    ) -> LatencyResult {
        // Convert nanosecond values to microseconds for display
        let min_latency = min_latency_ns as f64 / 1000.0;
        let max_latency = max_latency_ns as f64 / 1000.0;

        // Calculate microsecond statistics from the histogram
        let avg_latency = histogram.mean() / 1000.0;
        let p50_latency = histogram.value_at_quantile(0.5) as f64 / 1000.0;
        let p99_latency = histogram.value_at_quantile(0.99) as f64 / 1000.0;
        let p999_latency = histogram.value_at_quantile(0.999) as f64 / 1000.0;

        // Calculate standard deviation in microseconds
        let stdev_latency = histogram.stdev() / 1000.0;

        LatencyResult {
            size: msg_size,
            iterations,
            min_latency,
            max_latency,
            typical_latency: p50_latency,
            avg_latency,
            stdev_latency,
            p99_latency,
            p999_latency,
        }
    }

    fn calculate_bandwidth_results(
        &self,
        msg_size: u32,
        iterations: u32,
        time: f64,
    ) -> BandwidthResult {
        let total_bytes = msg_size as u64 * iterations as u64;
        let bytes_per_second = total_bytes as f64 / time;

        BandwidthResult {
            size: msg_size,
            iterations,
            bandwidth: Byte::from_f64(bytes_per_second)
                .unwrap()
                .get_adjusted_unit(byte_unit::Unit::Gbit)
                .get_value(),
            msg_rate: (iterations as f64) / time / 1_000_000.0,
            time: format!("{:.2}", time),
        }
    }

    // Check if flow control allows sending
    #[inline]
    fn check_send_credits(&self, fc_context: &Option<flow_context::FlowControlContext>) -> bool {
        // If flow control is not enabled, always allow
        if fc_context.is_none() {
            return true;
        }

        // Otherwise, check credits
        fc_context
            .as_ref()
            .unwrap()
            .get_sender()
            .is_none_or(|s| s.has_credit())
    }

    // Update credit after sending
    #[inline]
    fn consume_send_credit(&self, fc_context: &Option<flow_context::FlowControlContext>) {
        if let Some(fc) = fc_context {
            if let Some(sender) = fc.get_sender() {
                sender.consume_credit();
            }
        }
    }

    // Check if should update credits after receiving
    #[inline]
    fn check_credit_update(
        &self,
        fc_context: &mut Option<flow_context::FlowControlContext>,
        qp: &mut GenericQueuePair<'_>,
    ) -> anyhow::Result<()> {
        if let Some(fc) = fc_context.as_mut() {
            if let Some(receiver) = fc.get_receiver() {
                // Process the completion and check if we should send an update
                if receiver.process_completion() {
                    // If we need to update, get the processed count
                    let processed = receiver.get_processed_count();
                    debug!("Sending credit update: {}", processed);

                    // Get memory info for the credit update
                    receiver.get_memory_info().and_then(|(rkey, addr)| {
                        // If we have memory info, post the update through RDMA WRITE
                        let mut send_guard = qp.start_post_send();

                        let wr_id = (0u64 << 32) | 0xFFFFFFFFu64; // Special marker for credit updates
                        let send_handle = send_guard
                            .construct_wr(wr_id, WorkRequestFlags::Signaled)
                            .setup_write(rkey, addr);

                        unsafe {
                            send_handle.setup_sge(
                                // Use the same MR for source and dest
                                rkey,
                                // Use the same address for source and dest
                                addr,
                                std::mem::size_of::<u32>() as u32,
                            );
                        }

                        send_guard.post().ok()?;
                        trace!("Posted credit update: {}", processed);
                        Some(())
                    });
                }
            }
        }
        Ok(())
    }

    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting {}", self.plan.test_name());

        // Get parameters from plan
        let device_name = self.plan.base().dev.as_deref();
        let iterations = self.plan.base().iters;
        let msg_sizes = &self.plan.base().msg_sizes;
        let tx_depth = self.plan.base().tx_depth;
        let qp_count = self.plan.base().threads;

        info!("Will test {} message sizes", msg_sizes.len());

        let ctx = Arc::new(open_device_context(device_name)?);

        // Determine test type based on operation and mode
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
        let buffer_size = tx_depth as usize * max_msg_size as usize * qp_count;

        // Determine which memory type to use based on parameters
        let memory_type = if self.plan.base().hugepages {
            info!("Using hugepages for memory allocations");
            MemoryType::Hugepages(HugepageConfig::new(buffer_size))
        } else {
            info!("Using aligned memory for allocations");
            MemoryType::Aligned(AlignedConfig::new(buffer_size, DEFAULT_CACHE_LINE_SIZE))
        };

        // Create memory using the factory pattern (single call)
        let memory = MemoryAllocator::allocate(memory_type)?;

        let pd = Arc::new(ctx.alloc_pd()?);

        // Setup flow control if enabled
        let rx_depth = self.plan.rx_depth().unwrap_or(512);
        let mut fc_context = self.setup_flow_control(&pd, rx_depth)?;
        let mr = unsafe {
            pd.reg_mr(
                memory.get_handle(),
                memory.size(),
                AccessFlags::LocalWrite | AccessFlags::RemoteWrite | AccessFlags::RelaxedOrdering,
            )?
        };

        let cq_depth = tx_depth * qp_count as u32;
        let cq: GenericCompletionQueue = ctx
            .create_cq_builder()
            .setup_wc_flags(CreateCompletionQueueWorkCompletionFlags::StandardFlags)
            .setup_cqe(cq_depth)
            .build_ex()?
            .into();

        let mut builder = pd.create_qp_builder();

        // Create a new histogram for all tests
        let mut histogram = hdrhistogram::Histogram::<u64>::new(3).unwrap();

        // Create placeholder QP details (will be updated during connection setup)
        let mut qp_details = Vec::with_capacity(qp_count);
        for i in 0..qp_count {
            qp_details.push(QueuePairDetail {
                qp_index: i as u32,
                local_qpn: 0, // Will be filled in after QP creation
                local_psn: random::generate_psn(),
                remote_qpn: 0, // Will be filled in after connection
                remote_psn: 0, // Will be filled in after connection
            });
        }

        // Create queue pairs
        let mut qps: Vec<GenericQueuePair> = Vec::with_capacity(qp_count);
        for _ in 0..qp_count {
            let qp = builder
                .setup_max_inline_data(128)
                .setup_send_cq(&cq)
                .setup_recv_cq(&cq)
                .setup_max_send_wr(tx_depth)
                .setup_max_recv_wr(512)
                .build_ex()?;

            qps.push(qp.into());
        }

        let (conn_result, mut session) = self
            .setup_connection(ctx.clone(), pd.clone(), &mut qps, &mr, &mut qp_details)
            .unwrap();

        // Create a shared DisplayOutput for consolidated results if using multiple sizes
        let multiple_sizes = msg_sizes.len() > 1;
        let mut shared_display = if multiple_sizes {
            // If we're using multiple sizes, we'll create one shared DisplayOutput for all results
            let config = TestConfiguration {
                device: ctx.name(),
                transport: ctx.transport_type().to_string(),
                qp_count: qp_count as u32,
                connection_type: "RC".to_string(),
                mtu: conn_result.actual_mtu,
                gid_type: format!("{:?}", conn_result.gid_type),
                rx_depth,
                tx_depth,
                post_list: self.plan.base().post_list,
                test_type,
            };

            Some(DisplayOutput::new(
                config,
                qp_details.clone(),
                vec![conn_result.local_gid, conn_result.remote_gid],
            ))
        } else {
            None
        };

        for &msg_size in msg_sizes {
            info!(message_size = msg_size, "Running test.");

            // Reset the histogram for each size
            histogram.reset();

            // Create individual test display for this message size (or reuse the shared one)
            let mut display = if shared_display.is_none() {
                // Only create new display if not using the shared one
                DisplayOutput::new(
                    TestConfiguration {
                        device: ctx.name(),
                        transport: ctx.transport_type().to_string(),
                        qp_count: qp_count as u32,
                        connection_type: "RC".to_string(),
                        mtu: conn_result.actual_mtu,
                        gid_type: format!("{:?}", conn_result.gid_type),
                        rx_depth,
                        tx_depth,
                        post_list: self.plan.base().post_list,
                        test_type,
                    },
                    qp_details.clone(),
                    vec![conn_result.local_gid, conn_result.remote_gid],
                )
            } else {
                // When using multiple-sizes mode, this is just a dummy display since we use shared_display
                DisplayOutput::new(
                    TestConfiguration {
                        device: ctx.name(),
                        transport: ctx.transport_type().to_string(),
                        qp_count: qp_count as u32,
                        connection_type: "RC".to_string(),
                        mtu: conn_result.actual_mtu,
                        gid_type: format!("{:?}", conn_result.gid_type),
                        rx_depth,
                        tx_depth,
                        post_list: self.plan.base().post_list,
                        test_type,
                    },
                    Vec::new(),
                    Vec::new(),
                )
            };

            let is_server = self.plan.base().server;
            let is_bidirectional = self.plan.base().bidir;
            let remote_mr = conn_result.remote_mr;

            let iterations_per_qp = iterations;
            let mut qp_iterations = vec![0u32; qp_count];
            let mut inflight_per_qp = vec![0u32; qp_count];
            let total_target_iterations = iterations_per_qp * qp_count as u32;

            if is_bidirectional || !is_server {
                let clock = Clock::new();
                let start_time = clock.now();

                // Execute the test based on operation type
                let is_write = self.plan.needs_remote_addr();

                let mut all_completed = false;

                let mut min_latency_ns: u64 = u64::MAX;
                let mut max_latency_ns: u64 = 0;
                let mut operation_start_time = None;

                while !all_completed {
                    // Post operations to all QPs that have space in their queue
                    for qp_idx in 0..qp_count {
                        // Skip if this QP has completed all iterations
                        if qp_iterations[qp_idx] >= iterations_per_qp {
                            continue;
                        }

                        // Check if flow control allows posting more operations
                        if !self.check_send_credits(&fc_context) {
                            continue;
                        }

                        // Post operations until tx_depth is reached or iterations are complete
                        while inflight_per_qp[qp_idx] < tx_depth
                            && qp_iterations[qp_idx] < iterations_per_qp
                            && self.check_send_credits(&fc_context)
                        {
                            // Get the number of WQEs to post in a single batch
                            let post_list = self
                                .plan.base()
                                .post_list
                                .min(
                                    // Don't post more than what's left for this QP
                                    iterations_per_qp - qp_iterations[qp_idx],
                                )
                                .min(
                                    // Don't post more than the available tx_depth
                                    tx_depth - inflight_per_qp[qp_idx],
                                ) as usize;

                            // Take timestamp for latency measurements
                            operation_start_time =
                                if is_latency { Some(clock.now()) } else { None };

                            // Create a single post guard
                            let mut guard = qps[qp_idx].start_post_send();

                            // Track addresses for cache-line aligned increments
                            let mut prev_local_addr = 0;
                            let mut prev_remote_addr = 0;

                            // Post up to post_list operations at once
                            for i in 0..post_list {
                                // Calculate buffer offset - each QP has its own buffer region
                                let buffer_region_size = tx_depth as usize * msg_size as usize;
                                let qp_offset = qp_idx * buffer_region_size;
                                let base_addr = mr.get_ptr() as u64 + qp_offset as u64;

                                // Set up cycle buffer size - will wrap around after this many bytes
                                let cycle_buffer_size = buffer_region_size as u32;

                                // Get current iteration for this QP
                                let current_iter = qp_iterations[qp_idx] + i as u32;

                                // Calculate local address with cache-line aligned incrementing
                                let local_addr = if i == 0 {
                                    // First operation in batch uses calculated offset
                                    let iter_offset =
                                        (current_iter % tx_depth) as usize * msg_size as usize;
                                    base_addr + iter_offset as u64
                                } else {
                                    // Subsequent operations increment with cache-line alignment
                                    increase_addr_with_alignment(
                                        prev_local_addr,
                                        msg_size,
                                        current_iter - 1,
                                        base_addr,
                                        cycle_buffer_size,
                                    )
                                };

                                // Save current address for next iteration
                                prev_local_addr = local_addr;

                                let wr_id = ((qp_idx as u64) << 32) | (current_iter as u64);

                                // For WRITE operations, use remote memory info
                                let send_handle = if is_write {
                                    // Calculate remote base address for this QP
                                    let remote_base_addr = remote_mr.addr + qp_offset as u64;

                                    // Calculate remote address with cache-line aligned incrementing
                                    let remote_addr = if i == 0 {
                                        // First operation in batch uses calculated offset
                                        let remote_iter_offset =
                                            (current_iter % tx_depth) as usize * msg_size as usize;
                                        remote_base_addr + remote_iter_offset as u64
                                    } else {
                                        // Subsequent operations increment with cache-line alignment
                                        increase_addr_with_alignment(
                                            prev_remote_addr,
                                            msg_size,
                                            current_iter - 1,
                                            remote_base_addr,
                                            cycle_buffer_size,
                                        )
                                    };

                                    // Save current remote address for next iteration
                                    prev_remote_addr = remote_addr;

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

                            // Consume flow control credit if enabled
                            self.consume_send_credit(&fc_context);

                            // Update the iteration and inflight counters
                            qp_iterations[qp_idx] += post_list as u32;
                            inflight_per_qp[qp_idx] += post_list as u32;
                        }
                    }

                    if is_latency {
                        let start_time = operation_start_time.unwrap();
                        self.wait_for_completion(
                            &cq,
                            start_time,
                            &mut histogram,
                            &mut min_latency_ns,
                            &mut max_latency_ns,
                            &mut inflight_per_qp,
                            &clock,
                        )?;

                        // Check flow control updates on receive completions
                        if is_server {
                            self.check_credit_update(&mut fc_context, &mut qps[0])?;
                        }
                    } else {
                        self.poll_completions(&cq, &mut inflight_per_qp)?;

                        // Check flow control updates on receive completions
                        if is_server {
                            self.check_credit_update(&mut fc_context, &mut qps[0])?;
                        }
                    }

                    all_completed = true;
                    for qp_idx in 0..qp_count {
                        if qp_iterations[qp_idx] < iterations_per_qp || inflight_per_qp[qp_idx] > 0
                        {
                            all_completed = false;
                            break;
                        }
                    }
                }

                let end_time = clock.now();
                let time = end_time.duration_since(start_time);

                let total_iterations: u32 = qp_iterations.iter().sum();
                assert_eq!(
                    total_iterations, total_target_iterations,
                    "Iteration count mismatch: expected {}, got {}",
                    total_target_iterations, total_iterations
                );

                if is_latency {
                    let lat_results = self.calculate_latency_results(
                        msg_size,
                        total_iterations,
                        &histogram,
                        min_latency_ns,
                        max_latency_ns,
                    );

                    if let Some(shared) = &mut shared_display {
                        // Add to collection for consolidated display at the end
                        shared.add_latency_result(lat_results.clone());
                    } else {
                        // Set single result (legacy mode)
                        display.set_latency_results(lat_results.clone());
                    }

                    if !is_bidirectional && !is_server {
                        // Convert to TestResults for sending to server
                        let test_results = TestResults {
                            test_type: crate::connection::exchange::TestType::Latency,
                            size: msg_size,
                            iterations: total_iterations,
                            time: format!("{:.2}", time.as_secs_f64()),
                            bandwidth_result: None,
                            latency_result: Some(lat_results),
                        };

                        // Use the existing session to send results
                        session.send_results(&test_results)?;
                    }
                } else {
                    let bw_results = self.calculate_bandwidth_results(
                        msg_size,
                        total_iterations,
                        time.as_secs_f64(),
                    );

                    if let Some(shared) = &mut shared_display {
                        // Add to collection for consolidated display at the end
                        shared.add_bandwidth_result(bw_results.clone());
                    } else {
                        // Set single result (legacy mode)
                        display.set_bandwidth_results(bw_results.clone());
                    }

                    // If client in unidirectional mode, send results to server
                    if !is_bidirectional && !is_server {
                        let test_results = TestResults {
                            test_type: crate::connection::exchange::TestType::Bandwidth,
                            size: msg_size,
                            iterations: total_iterations,
                            time: format!("{:.2}", time.as_secs_f64()),
                            bandwidth_result: Some(bw_results),
                            latency_result: None,
                        };

                        // Use the existing session to send results
                        session.send_results(&test_results)?;
                    }
                }

                // Display individual results immediately if not in multi-sizes mode
                if shared_display.is_none() {
                    display.display();
                }
            } else {
                // Server in unidirectional mode
                debug!("Server ready for client operations");

                // Wait for results from client
                let test_results = session.receive_results()?;

                match test_results.test_type {
                    crate::connection::exchange::TestType::Latency => {
                        if let Some(lat_results) = test_results.latency_result {
                            info!(
                                test_type = format!("{:?}", test_results.test_type),
                                message_size = test_results.size,
                                iterations = test_results.iterations,
                                avg_latency = format!("{:.5} us", lat_results.avg_latency),
                                min_latency = format!("{:.5} us", lat_results.min_latency),
                                max_latency = format!("{:.5} us", lat_results.max_latency),
                                p99_latency = format!("{:.5} us", lat_results.p99_latency),
                                p999_latency = format!("{:.5} us", lat_results.p999_latency),
                                "Received latency test results from client."
                            );

                            if let Some(shared) = &mut shared_display {
                                shared.add_latency_result(lat_results);
                            } else {
                                display.set_latency_results(lat_results);
                            }
                        }
                    }

                    crate::connection::exchange::TestType::Bandwidth => {
                        if let Some(bw_results) = test_results.bandwidth_result {
                            info!(
                                test_type = format!("{:?}", test_results.test_type),
                                message_size = test_results.size,
                                iterations = test_results.iterations,
                                bandwidth = format!("{:.5} Gbps", bw_results.bandwidth),
                                mpps = format!("{:.5} Mpps", bw_results.msg_rate),
                                "Received bandwidth test results from client."
                            );

                            if let Some(shared) = &mut shared_display {
                                shared.add_bandwidth_result(bw_results);
                            } else {
                                display.set_bandwidth_results(bw_results);
                            }
                        }
                    }
                }

                // Display individual results immediately if not in multi-sizes mode
                if shared_display.is_none() {
                    display.display();
                }
            }
        }

        // Close the session after all tests are complete
        session.close()?;

        // Display consolidated results if in multi-sizes mode
        if let Some(shared) = shared_display {
            println!("\n{}", "-".repeat(80));
            println!("Consolidated results for all message sizes:");
            println!("{}", "-".repeat(80));
            shared.display();
        }

        Ok(())
    }
}
