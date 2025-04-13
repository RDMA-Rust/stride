use crate::connection::exchange::ConnectionSetupResult;
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

use std::sync::Arc;
use std::time::Duration;

use crate::cli::context::CommandContext;
use crate::connection::exchange::TestResults;
use crate::connection::session::ConnectionSession;
use crate::connection::ConnectionParams;
use crate::connection::EndpointRole;
use crate::context::device::open_device_context;
use crate::memory::system::SystemMemory;
use crate::memory::MemoryOps;
use crate::utils::display::{
    BandwidthResult, DisplayOutput, LatencyResult, QueuePairDetail, TestConfiguration, TestType,
};
use crate::utils::random;

pub struct TestRunner<T: CommandContext> {
    params: T,
}

impl<T: CommandContext> TestRunner<T> {
    pub fn new(params: T) -> Self {
        Self { params }
    }

    fn setup_connection<'a>(
        &self,
        ctx: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain<'a>>,
        qps: &mut [GenericQueuePair],
        mr: &MemoryRegion,
        qp_details: &mut [QueuePairDetail],
    ) -> Result<(ConnectionSetupResult, ConnectionSession<'a>)> {
        let gid_index = self.params.gid_index().unwrap_or(0);
        let server_mode = self.params.server_mode();

        // Create connection parameters
        let mut conn_params = ConnectionParams::default();
        conn_params.role = if server_mode {
            println!("Running in server mode");
            EndpointRole::Server
        } else {
            println!("Running in client mode");
            EndpointRole::Client
        };

        // Adjust timeout based on QP timeout parameter
        let timeout_factor = self.params.qp_timeout();
        conn_params.timeout = Duration::from_micros(4 * (1u64 << timeout_factor));

        let mut session =
            ConnectionSession::new("tcp", ctx.clone(), pd.clone(), conn_params, gid_index)?;

        // Initialize the connection session
        let _ = session.initialize();

        // Establish connection
        let address = if server_mode {
            format!("0.0.0.0:{}", self.params.port().unwrap_or(18515))
        } else {
            let target = self.params.address().unwrap_or_else(|| {
                println!("No target address specified, using localhost");
                "127.0.0.1".to_string()
            });

            format!("{}:{}", target, self.params.port().unwrap_or(18515))
        };

        println!("Establishing connection via {}", address);
        session.establish_connection(&address)?;
        let actual_mtu = 4096;

        let gid_entry = ctx.query_gid_ex(1, gid_index as u32)?;
        let gid_type = gid_entry.gid_type();
        let local_gid = gid_entry.gid();
        let mut remote_gid = Gid::default();

        // Setup each queue pair
        println!("Setting up {} queue pairs", qps.len());
        for (i, (qp, detail)) in qps.iter_mut().zip(qp_details.iter_mut()).enumerate() {
            println!("Setting up QP #{}", i);
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

            println!(
                "QP #{} setup complete: Local QPN: 0x{:x}, Remote QPN: 0x{:x}, Remote PSN: 0x{:x}",
                i, detail.local_qpn, detail.remote_qpn, detail.remote_psn
            );
        }

        // Exchange memory regions after QP setup
        println!("Exchanging memory region information...");
        let remote_mr =
            session.exchange_memory_regions(mr.get_ptr() as u64, mr.rkey(), mr.region_len())?;

        session.synchronize_qps()?;

        // Close connection
        // println!("Connection setup complete, closing control connection");
        // session.close()?;

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

    // Helper methods to improve readability
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
                    while let Some(wc) = poller.next() {
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
                            // let latency_ns = if wc.wc_flags()
                            //     & CreateCompletionQueueWorkCompletionFlags::CompletionTimestamp.bi
                            //     != 0
                            // {
                            //     // Use hardware timestamp if available
                            //     wc.completion_timestamp() as u64
                            // } else {
                            // Fall back to software timing
                            let latency_ns =
                                completion_time.duration_since(start_time).into_nanos();
                            // };

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

    fn poll_completions(
        &self,
        cq: &GenericCompletionQueue,
        inflight_per_qp: &mut [u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        match cq.start_poll() {
            Ok(mut poller) => {
                while let Some(wc) = poller.next() {
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
                }
            }
            Err(_) => {}
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

    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting {}", self.params.operation_name());

        // Default device fallback
        let device_name = self.params.device();
        let iterations = self.params.iterations();
        let msg_size = self.params.message_size();
        let tx_depth = self.params.tx_depth().unwrap_or(512);
        let qp_count = self.params.qp_count().unwrap_or(1) as usize;

        let ctx = Arc::new(open_device_context(device_name)?);
        let mut histogram = hdrhistogram::Histogram::<u64>::new(3).unwrap();

        // Determine test type
        let test_type = if self.params.operation_name().contains("SEND") {
            if self.params.operation_name().contains("latency") {
                TestType::SendLatency
            } else {
                TestType::SendBandwidth
            }
        } else if self.params.operation_name().contains("WRITE") {
            if self.params.operation_name().contains("latency") {
                TestType::WriteLatency
            } else {
                TestType::WriteBandwidth
            }
        } else {
            if self.params.operation_name().contains("latency") {
                TestType::ReadLatency
            } else {
                TestType::ReadBandwidth
            }
        };

        let is_latency = test_type.is_latency();
        let tx_depth = if is_latency { 1 } else { tx_depth };

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

        let buffer_size = tx_depth as usize * msg_size as usize * qp_count;
        let memory = SystemMemory::new(buffer_size, None)?;
        let pd = Arc::new(ctx.alloc_pd()?);
        let mr = unsafe {
            pd.reg_mr(
                memory.get_handle(),
                memory.size(),
                AccessFlags::LocalWrite | AccessFlags::RemoteWrite | AccessFlags::RelaxedOrdering,
            )?
        };

        let cq_depth = tx_depth as u32 * qp_count as u32;
        let cq: GenericCompletionQueue = ctx
            .create_cq_builder()
            .setup_wc_flags(CreateCompletionQueueWorkCompletionFlags::StandardFlags)
            .setup_cqe(cq_depth)
            .build_ex()?
            .into();

        let mut builder = pd.create_qp_builder();
        let qp_count = self.params.qp_count().unwrap_or(1);
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

        // Create test configuration
        let config = TestConfiguration {
            device: ctx.name(),
            transport: ctx.transport_type().to_string(),
            qp_count: qp_count as u32,
            connection_type: "RC".to_string(),
            mtu: conn_result.actual_mtu,
            gid_type: format!("{:?}", conn_result.gid_type),
            rx_depth: self.params.rx_depth().unwrap_or(512),
            tx_depth,
            test_type,
        };

        let gid_info = vec![conn_result.local_gid, conn_result.remote_gid];
        let mut display = DisplayOutput::new(config, qp_details, gid_info);

        let is_server = self.params.server_mode();
        let is_bidirectional = self.params.bidirectional();
        let remote_mr = conn_result.remote_mr;

        let iterations_per_qp = iterations;
        let mut qp_iterations = vec![0u32; qp_count];
        let mut inflight_per_qp = vec![0u32; qp_count];
        let total_target_iterations = iterations_per_qp * qp_count as u32;

        if is_bidirectional || !is_server {
            let clock = Clock::new();
            let start_time = clock.now();

            // Execute the test based on operation type
            let is_write = self.params.operation_name().contains("WRITE");

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

                    // Post operations until tx_depth is reached or iterations are complete
                    while inflight_per_qp[qp_idx] < tx_depth
                        && qp_iterations[qp_idx] < iterations_per_qp
                    {
                        operation_start_time = if is_latency { Some(clock.now()) } else { None };

                        let mut guard = qps[qp_idx].start_post_send();

                        // Calculate buffer offset - each QP has its own buffer region
                        let buffer_region_size = tx_depth as usize * msg_size as usize;
                        let qp_offset = qp_idx * buffer_region_size;
                        let iter_offset =
                            (qp_iterations[qp_idx] % tx_depth) as usize * msg_size as usize;
                        let total_offset = qp_offset + iter_offset;

                        let local_addr = mr.get_ptr() as u64 + total_offset as u64;

                        let wr_id = ((qp_idx as u64) << 32) | (qp_iterations[qp_idx] as u64);

                        // For WRITE operations, use remote memory info
                        let send_handle = if is_write {
                            // Remote memory layout should match local layout
                            let remote_offset = total_offset % conn_result.remote_mr.size;
                            let remote_addr = remote_mr.addr + remote_offset as u64;

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

                        guard.post()?;

                        qp_iterations[qp_idx] += 1;
                        inflight_per_qp[qp_idx] += 1;
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
                } else {
                    self.poll_completions(&cq, &mut inflight_per_qp)?;
                }

                all_completed = true;
                for qp_idx in 0..qp_count {
                    if qp_iterations[qp_idx] < iterations_per_qp || inflight_per_qp[qp_idx] > 0 {
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
                display.set_latency_results(lat_results.clone());

                if !is_bidirectional && !is_server {
                    // Convert BandwidthResult to TestResults
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
                display.set_bandwidth_results(bw_results.clone());

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

            display.display();
        } else {
            // Server in unidirectional mode
            println!("Server ready for client operations");

            // Wait for results from client
            let test_results = session.receive_results()?;

            match test_results.test_type {
                crate::connection::exchange::TestType::Latency => {
                    if let Some(lat_results) = test_results.latency_result {
                        display.set_latency_results(lat_results);
                    }
                }

                crate::connection::exchange::TestType::Bandwidth => {
                    if let Some(bw_results) = test_results.bandwidth_result {
                        display.set_bandwidth_results(bw_results);
                    }
                }
            }

            display.display();
        }

        session.close()?;

        Ok(())
    }
}
