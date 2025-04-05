use crate::connection::exchange::MemoryRegionInfo;
use anyhow::Result;
use byte_unit::{Byte, UnitType};
use quanta::Clock;
use sideway::ibverbs::address::AddressHandleAttribute;
use sideway::ibverbs::completion::{
    CreateCompletionQueueWorkCompletionFlags, GenericCompletionQueue, WorkCompletionStatus,
};
use sideway::ibverbs::device::DeviceInfo;
use sideway::ibverbs::device_context::DeviceContext;
use sideway::ibverbs::device_context::Mtu;
use sideway::ibverbs::memory_region::MemoryRegion;
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::{
    GenericQueuePair, PostSendGuard, QueuePair, QueuePairAttribute, QueuePairState,
    SetScatterGatherEntry, WorkRequestFlags,
};
use sideway::ibverbs::AccessFlags;

use std::sync::Arc;
use std::time::Duration;

use crate::cli::context::CommandContext;
use crate::connection::session::ConnectionSession;
use crate::connection::ConnectionParams;
use crate::connection::EndpointRole;
use crate::context::device::open_device_context;
use crate::memory::system::SystemMemory;
use crate::memory::MemoryOps;
use crate::utils::display::{
    BandwidthResult, DisplayOutput, QueuePairDetail, TestConfiguration, TestType,
};
use crate::utils::random;

pub struct TestRunner<T: CommandContext> {
    params: T,
}

impl<T: CommandContext> TestRunner<T> {
    pub fn new(params: T) -> Self {
        Self { params }
    }

    fn setup_connection(
        &self,
        ctx: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain>,
        qps: &mut [GenericQueuePair],
        mr: &MemoryRegion,
    ) -> Result<MemoryRegionInfo> {
        let gid_index = self.params.gid_index().unwrap_or(0);
        let server_mode = self.params.server_mode().unwrap_or(false);

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
        session.initialize();

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

        // Setup each queue pair
        println!("Setting up {} queue pairs", qps.len());
        for (i, qp) in qps.iter_mut().enumerate() {
            println!("Setting up QP #{}", i);
            let remote_data = session.setup_queue_pair(qp)?;
            println!(
                "QP #{} setup complete: Local QPN: 0x{:x}, Remote QPN: 0x{:x}, Remote PSN: 0x{:x}",
                i,
                qp.qp_number(),
                remote_data.qp_number,
                remote_data.psn
            );
        }

        // Exchange memory regions after QP setup
        println!("Exchanging memory region information...");
        let remote_mr =
            session.exchange_memory_regions(mr.get_ptr() as u64, mr.rkey(), mr.region_len())?;

        session.synchronize_qps()?;

        // Close connection
        println!("Connection setup complete, closing control connection");
        session.close()?;

        Ok(remote_mr)
    }

    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting {}", self.params.operation_name());

        // Default device fallback
        let device_name = self.params.device().unwrap_or("mlx5_1");
        let iterations = self.params.iterations();
        let msg_size = self.params.message_size();
        let tx_depth = self.params.tx_depth().unwrap_or(512);
        let qp_count = self.params.qp_count().unwrap_or(1) as usize;

        let ctx = Arc::new(open_device_context(device_name)?);
        let gid = ctx.query_gid_ex(1, 0)?;
        let gid_2 = ctx.query_gid_ex(1, 1)?;

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

        // Create test configuration
        let config = TestConfiguration {
            device: ctx.name(),
            transport: "IB".to_string(),
            qp_count: qp_count as u32,
            connection_type: "RC".to_string(),
            mtu: 4096,
            gid_type: format!("{:?}", gid.gid_type()),
            rx_depth: self.params.rx_depth().unwrap_or(512),
            tx_depth,
            test_type,
        };

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

        let gid_info = vec![gid, gid_2];
        let mut display = DisplayOutput::new(config, qp_details, gid_info);

        let memory = SystemMemory::new(tx_depth as usize * msg_size as usize, None)?;
        let pd = Arc::new(ctx.alloc_pd()?);
        let mr = unsafe {
            pd.reg_mr(
                memory.get_handle(),
                memory.size(),
                AccessFlags::LocalWrite | AccessFlags::RemoteWrite,
            )?
        };

        // println!("MR registered {:#?}", mr);

        let cq: GenericCompletionQueue = ctx
            .create_cq_builder()
            .setup_wc_flags(CreateCompletionQueueWorkCompletionFlags::StandardFlags)
            .setup_cqe(512)
            .build_ex()?
            .into();

        let mut builder = pd.create_qp_builder();
        let psn = random::generate_psn();

        let qp_count = self.params.qp_count().unwrap_or(1);
        let mut qps: Vec<GenericQueuePair> = Vec::with_capacity(qp_count);

        for _ in 0..qp_count {
            let mut builder = pd.create_qp_builder();
            let mut qp = builder
                .setup_max_inline_data(128)
                .setup_send_cq(&cq)
                .setup_recv_cq(&cq)
                .setup_max_send_wr(512)
                .setup_max_recv_wr(512)
                .build_ex()?;

            qps.push(qp.into());
        }

        let remote_mr = self
            .setup_connection(ctx.clone(), pd.clone(), &mut qps, &mr)
            .unwrap();

        // let mut attr = QueuePairAttribute::new();
        // attr.setup_state(QueuePairState::Init)
        //     .setup_pkey_index(0)
        //     .setup_port(1)
        //     .setup_access_flags(AccessFlags::LocalWrite | AccessFlags::RemoteWrite);
        // qp.modify(&attr)?;

        // let mut attr = QueuePairAttribute::new();
        // attr.setup_state(QueuePairState::ReadyToReceive)
        //     .setup_path_mtu(Mtu::Mtu4096)
        //     .setup_dest_qp_num(qp.qp_number())
        //     .setup_rq_psn(psn)
        //     .setup_max_dest_read_atomic(0)
        //     .setup_min_rnr_timer(0);
        // let mut ah_attr = AddressHandleAttribute::new();

        // ah_attr
        //     .setup_dest_lid(1)
        //     .setup_port(1)
        //     .setup_service_level(42)
        //     .setup_grh_src_gid_index(0)
        //     .setup_grh_dest_gid(&ctx.query_gid(1, 0)?)
        //     .setup_grh_hop_limit(255);
        // attr.setup_address_vector(&ah_attr);
        // qp.modify(&attr)?;

        // let mut attr = QueuePairAttribute::new();
        // attr.setup_state(QueuePairState::ReadyToSend)
        //     .setup_sq_psn(psn)
        //     .setup_timeout(12)
        //     .setup_retry_cnt(7)
        //     .setup_rnr_retry(7)
        //     .setup_max_read_atomic(0);
        // qp.modify(&attr)?;

        let mut cur_iter: u32 = 0;
        let mut inflight = 0;

        let clock = Clock::new();
        let start_time = clock.now();

        // Execute the test based on operation type
        let is_write = self.params.operation_name().contains("WRITE");

        while cur_iter < iterations {
            while inflight < tx_depth {
                let mut guard = qps[0].start_post_send();

                let offset = (cur_iter % tx_depth) as usize * msg_size as usize;
                let addr = mr.get_ptr() as u64 + offset as u64;

                let send_handle = if is_write {
                    // Use remote memory information for WRITE operations
                    let remote_offset = offset % remote_mr.size;
                    let remote_addr = remote_mr.addr + remote_offset as u64;
                    guard
                        .construct_wr(cur_iter as _, WorkRequestFlags::Signaled)
                        .setup_write(remote_mr.rkey, remote_addr)
                } else {
                    guard
                        .construct_wr(cur_iter as _, WorkRequestFlags::Signaled)
                        .setup_send()
                };

                unsafe {
                    send_handle.setup_sge(mr.lkey(), addr, msg_size);
                }

                guard.post()?;

                cur_iter += 1;
                inflight += 1;
            }

            match cq.start_poll() {
                Ok(mut poller) => {
                    while let Some(wc) = poller.next() {
                        if wc.status() != WorkCompletionStatus::Success as u32 {
                            return Err(format!(
                                "Failed status {:#?} ({}) for wr_id {}",
                                Into::<WorkCompletionStatus>::into(wc.status()),
                                wc.status(),
                                wc.wr_id()
                            )
                            .into());
                        }
                        inflight -= 1;
                    }
                }
                Err(_) => continue,
            }
        }

        let end_time = clock.now();
        let time = end_time.duration_since(start_time);
        let bytes = msg_size as u64 * cur_iter as u64;
        let bytes_per_second = bytes as f64 / time.as_secs_f64();

        let results = BandwidthResult {
            size: msg_size,
            iterations: cur_iter,
            bandwidth: Byte::from_f64(bytes_per_second)
                .unwrap()
                .get_appropriate_unit(UnitType::Binary)
                .get_value(),
            msg_rate: (cur_iter as f64) / time.as_secs_f64() / 1_000_000.0,
            time: format!("{:.2}", time.as_secs_f64()),
        };
        display.set_bandwidth_results(results);

        display.display();
        Ok(())
    }
}
