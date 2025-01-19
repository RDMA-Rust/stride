use anyhow::Result;
use byte_unit::{Byte, UnitType};
use quanta::Clock;
use sideway::ibverbs::address::AddressHandleAttribute;
use sideway::ibverbs::completion::{CreateCompletionQueueWorkCompletionFlags, GenericCompletionQueue, WorkCompletionStatus};
use sideway::ibverbs::device_context::Mtu;
use sideway::ibverbs::device::DeviceInfo;
use sideway::ibverbs::queue_pair::{
    PostSendGuard, QueuePairAttribute, QueuePairState, SetScatterGatherEntry,
    WorkRequestFlags, QueuePair,
};
use sideway::ibverbs::AccessFlags;

use crate::context::device::open_device_context;
use crate::memory::system::SystemMemory;
use crate::memory::MemoryOps;
use crate::utils::display::{BandwidthResult, DisplayOutput, QueuePairDetail, TestConfiguration};
use crate::utils::random;
use crate::cli::context::CommandContext;

pub struct TestRunner<T: CommandContext> {
    params: T,
}

impl<T: CommandContext> TestRunner<T> {
    pub fn new(params: T) -> Self {
        Self { params }
    }

    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting {}", self.params.operation_name());

        // Default device fallback
        let device_name = self.params.device().unwrap_or("mlx5_1");
        let iterations = self.params.iterations();
        let msg_size = self.params.message_size();
        let tx_depth = self.params.tx_depth().unwrap_or(512);

        let ctx = open_device_context(device_name)?;
        let gid = ctx.query_gid_ex(1, 0)?;
        let gid_2 = ctx.query_gid_ex(1, 1)?;

        let config = TestConfiguration {
            device: ctx.name(),
            transport: "IB".to_string(),
            qp_count: 2,
            connection_type: "RC".to_string(),
            mtu: 4096,
            gid_type: format!("{:?}", gid.gid_type()),
            rx_depth: self.params.rx_depth().unwrap_or(512),
            tx_depth,
        };

        // Create QP details with random PSNs
        let qp_details = vec![
            QueuePairDetail {
                qp_index: 0,
                local_qpn: 0x04fc,
                local_psn: random::generate_psn(),
                remote_qpn: 0x04fc,
                remote_psn: random::generate_psn(),
            },
            QueuePairDetail {
                qp_index: 1,
                local_qpn: 0x04fd,
                local_psn: random::generate_psn(),
                remote_qpn: 0x04fd,
                remote_psn: random::generate_psn(),
            },
        ];

        let gid_info = vec![gid, gid_2];
        let mut display = DisplayOutput::new(config, qp_details, gid_info);

        let memory = SystemMemory::new(tx_depth as usize * msg_size as usize, None)?;
        let pd = ctx.alloc_pd()?;
        let mr = unsafe {
            pd.reg_mr(
                memory.get_handle(),
                memory.size(),
                AccessFlags::LocalWrite | AccessFlags::RemoteWrite,
            )?
        };

        println!("MR registered {:#?}", mr);

        let cq: GenericCompletionQueue = ctx
            .create_cq_builder()
            .setup_wc_flags(CreateCompletionQueueWorkCompletionFlags::StandardFlags)
            .setup_cqe(512)
            .build_ex()?
            .into();

        let mut builder = pd.create_qp_builder();

        let mut qp = builder
            .setup_max_inline_data(128)
            .setup_send_cq(&cq)
            .setup_recv_cq(&cq)
            .setup_max_send_wr(512)
            .setup_max_recv_wr(512)
            .build_ex()?;

        let mut attr = QueuePairAttribute::new();
        attr.setup_state(QueuePairState::Init)
            .setup_pkey_index(0)
            .setup_port(1)
            .setup_access_flags(AccessFlags::LocalWrite | AccessFlags::RemoteWrite);
        qp.modify(&attr)?;

        let mut attr = QueuePairAttribute::new();
        attr.setup_state(QueuePairState::ReadyToReceive)
            .setup_path_mtu(Mtu::Mtu4096)
            .setup_dest_qp_num(qp.qp_number())
            .setup_rq_psn(random::generate_psn())
            .setup_max_dest_read_atomic(0)
            .setup_min_rnr_timer(0);
        let mut ah_attr = AddressHandleAttribute::new();

        ah_attr
            .setup_dest_lid(1)
            .setup_port(1)
            .setup_service_level(42)
            .setup_grh_src_gid_index(0)
            .setup_grh_dest_gid(&ctx.query_gid(1, 0)?)
            .setup_grh_hop_limit(255);
        attr.setup_address_vector(&ah_attr);
        qp.modify(&attr)?;

        let mut attr = QueuePairAttribute::new();
        attr.setup_state(QueuePairState::ReadyToSend)
            .setup_sq_psn(random::generate_psn())
            .setup_timeout(12)
            .setup_retry_cnt(7)
            .setup_rnr_retry(7)
            .setup_max_read_atomic(0);
        qp.modify(&attr)?;

        let mut cur_iter: u32 = 0;
        let mut inflight = 0;

        let clock = Clock::new();
        let start_time = clock.now();

        // Execute the test based on operation type
        let is_write = self.params.operation_name().contains("WRITE");

        while cur_iter < iterations {
            while inflight < tx_depth {
                let mut guard = qp.start_post_send();

                let offset = (cur_iter % tx_depth) as usize * msg_size as usize;
                let addr = mr.get_ptr() as u64 + offset as u64;

                let send_handle = if is_write {
                    guard
                        .construct_wr(cur_iter as _, WorkRequestFlags::Signaled)
                        .setup_write(mr.rkey(), addr)
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
                            ).into());
                        }
                        inflight -= 1;
                    }
                }
                Err(_) => continue
            }
        }

        let end_time = clock.now();
        let time = end_time.duration_since(start_time);
        let bytes = msg_size as u64 * cur_iter as u64;
        let bytes_per_second = bytes as f64 / time.as_secs_f64();

        let results = BandwidthResult {
            size: msg_size,
            iterations: cur_iter,
            bandwidth: Byte::from_f64(bytes_per_second).unwrap().get_appropriate_unit(UnitType::Binary).get_value(),
            msg_rate: (cur_iter as f64) / time.as_secs_f64() / 1_000_000.0,
            time: format!("{:.2}", time.as_secs_f64()),
        };
        display.set_results(results);

        display.display();
        Ok(())
    }
}
