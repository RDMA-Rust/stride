use clap::{builder, Args, Parser, Subcommand};
use context::device::open_device_context;
use postcard::fixint::le;
use sideway::ibverbs::address::AddressHandleAttribute;
use sideway::ibverbs::completion::{CreateCompletionQueueWorkCompletionFlags, GenericCompletionQueue, WorkCompletionStatus};
use sideway::ibverbs::device::{DeviceInfo, DeviceList};
use sideway::ibverbs::device_context::Mtu;
use sideway::ibverbs::queue_pair::{
    PostSendGuard, QueuePair, QueuePairAttribute, QueuePairState, SetScatterGatherEntry,
    WorkRequestFlags,
};
use sideway::ibverbs::AccessFlags;
use utils::display::{BandwidthResult, DisplayOutput, QueuePairDetail, TestConfiguration};
use byte_unit::{Byte, UnitType};

mod connection;
mod context;
mod memory;
mod operations;
mod runners;
mod transport;
mod utils;

use memory::system::SystemMemory;
use memory::MemoryOps;

#[derive(Parser)]
#[command(name = "stride")]
#[command(about = "RDMA benchmark tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand)]
    Run(RunCommands),
    Bench(BenchArgs),
    Probe(ProbeArgs),
}

#[derive(Subcommand)]
enum RunCommands {
    Send(SendArgs),
    Write(WriteArgs),
    Read(ReadArgs),
}
#[derive(Args)]
struct CommonArgs {
    /// Use IB device <DEVICE> [default: first device found]
    #[arg(long, short = 'd')]
    device: Option<String>,
    /// Test uses GID with GID index taken from command
    #[arg(long, short = 'x')]
    gid_index: Option<u32>,
    /// Number of exchanges (at least 100)
    #[arg(long, short = 'n', default_value_t = 1000)]
    iters: u32,
    /// Message size in bytes
    #[arg(long, short = 's', default_value_t = 65536)]
    msg_size: u32,
    /// QP timeout = (4 us) * (2 ^ timeout)
    #[arg(long, short = 'u', default_value_t = 14)]
    qp_timeout: u8,
}

#[derive(Args)]
struct SendArgs {
    #[command(subcommand)]
    metric: SendMetricType,
}

#[derive(Args)]
struct WriteArgs {
    #[command(subcommand)]
    metric: WriteMetricType,
}

#[derive(Args)]
struct ReadArgs {
    #[command(subcommand)]
    metric: ReadMetricType,
}

#[derive(Subcommand)]
enum SendMetricType {
    /// Bandwidth test with RDMA Send transactions
    #[command(alias = "bw")]
    Bandwidth(SendBandwidthArgs),
    /// Latency test with RDMA Send transactions
    #[command(alias = "lat")]
    Latency(SendLatencyArgs),
}

#[derive(Subcommand)]
enum WriteMetricType {
    /// Bandwidth test with RDMA Write transactions
    #[command(alias = "bw")]
    Bandwidth(WriteBandwidthArgs),
    /// Latency test with RDMA Write transactions
    #[command(alias = "lat")]
    Latency(WriteLatencyArgs),
}

#[derive(Subcommand)]
enum ReadMetricType {
    /// Bandwidth test with RDMA Read transactions
    #[command(alias = "bw")]
    Bandwidth(ReadBandwidthArgs),
    /// Latency test with RDMA Read transactions
    #[command(alias = "lat")]
    Latency(ReadLatencyArgs),
}

#[derive(Args)]
struct SendBandwidthArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Size of Tx queue
    #[arg(long, short = 't', default_value_t = 128)]
    tx_depth: u32,
    /// Size of Rx queue
    #[arg(long)]
    rx_depth: Option<u32>,
    /// Use send-with-immediate verb instead of send
    #[arg(long)]
    imm_data: bool,
}

#[derive(Args)]
struct SendLatencyArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Size of Tx queue
    #[arg(long)]
    tx_depth: Option<u32>,
    /// Size of Rx queue
    #[arg(long)]
    rx_depth: Option<u32>,
    /// Use send-with-immediate verb instead of send
    #[arg(long)]
    imm_data: bool,
}

#[derive(Args)]
struct WriteBandwidthArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Size of Tx queue
    #[arg(long, short = 't', default_value_t = 128)]
    tx_depth: u32,
    /// Use write-with-immediate verb instead of write
    #[arg(long)]
    imm_data: bool,
}

#[derive(Args)]
struct WriteLatencyArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Size of Tx queue
    #[arg(long, short = 't')]
    tx_depth: Option<u32>,
    /// Use write-with-immediate verb instead of write
    #[arg(long)]
    imm_data: bool,
}

#[derive(Args)]
struct ReadBandwidthArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    tx_depth: Option<u32>,
}

#[derive(Args)]
struct ReadLatencyArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    tx_depth: Option<u32>,
}

#[derive(Args)]
struct BenchArgs {
    #[arg(long)]
    numa: bool,
}

#[derive(Args)]
struct ProbeArgs {
    #[arg(long)]
    numa: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run(args) => {
            let (iters, msg_size, tx_depth) = match args {
                RunCommands::Send(send_args) => match send_args.metric {
                    SendMetricType::Bandwidth(args) => (args.common.iters, args.common.msg_size, args.tx_depth),
                    SendMetricType::Latency(args) => (args.common.iters, args.common.msg_size, args.tx_depth.unwrap_or(512)),
                },
                RunCommands::Write(write_args) => match write_args.metric {
                    WriteMetricType::Bandwidth(args) => (args.common.iters, args.common.msg_size, args.tx_depth),
                    WriteMetricType::Latency(args) => (args.common.iters, args.common.msg_size, args.tx_depth.unwrap_or(512)),
                },
                RunCommands::Read(read_args) => match read_args.metric {
                    ReadMetricType::Bandwidth(args) => (args.common.iters, args.common.msg_size, args.tx_depth.unwrap_or(512)),
                    ReadMetricType::Latency(args) => (args.common.iters, args.common.msg_size, args.tx_depth.unwrap_or(512)),
                },
            };

            let ctx = open_device_context("mlx5_1").unwrap();
            let gid = ctx.query_gid_ex(1, 0).unwrap();
            let gid_2 = ctx.query_gid_ex(1, 1).unwrap();

            let config = TestConfiguration {
                device: ctx.name(),
                transport: "IB".to_string(),
                qp_count: 2,
                connection_type: "RC".to_string(),
                mtu: 4096,
                gid_type: format!("{:?}", gid.gid_type()),
                rx_depth: 512,
                tx_depth: tx_depth,
            };

            let qp_details = vec![
                QueuePairDetail {
                    qp_index: 0,
                    local_qpn: 0x04fc,
                    local_psn: 0xc46be7,
                    remote_qpn: 0x04fc,
                    remote_psn: 0x7d6053,
                },
                QueuePairDetail {
                    qp_index: 1,
                    local_qpn: 0x04fd,
                    local_psn: 0xc46be8,
                    remote_qpn: 0x04fd,
                    remote_psn: 0x7d6054,
                },
            ];

            let gid_info = vec![gid, gid_2];

            let mut display = DisplayOutput::new(config, qp_details, gid_info);

            let memory = SystemMemory::new((tx_depth as usize * msg_size as usize) as usize, None).unwrap();
            let pd = ctx.alloc_pd().unwrap();
            let mr = unsafe {
                pd.reg_mr(
                    memory.get_handle(),
                    memory.size(),
                    AccessFlags::LocalWrite | AccessFlags::RemoteWrite,
                )
                .unwrap()
            };

            println!("MR registered {:#?}", mr);

            let cq: GenericCompletionQueue = ctx
                .create_cq_builder()
                .setup_wc_flags(CreateCompletionQueueWorkCompletionFlags::StandardFlags)
                .setup_cqe(512)
                .build_ex()
                .unwrap()
                .into();

            let mut builder = pd.create_qp_builder();

            let mut qp = builder
                .setup_max_inline_data(128)
                .setup_send_cq(&cq)
                .setup_recv_cq(&cq)
                .setup_max_send_wr(512)
                .setup_max_recv_wr(512)
                .build_ex()
                .unwrap();

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
                .setup_rq_psn(utils::random::generate_psn())
                .setup_max_dest_read_atomic(0)
                .setup_min_rnr_timer(0);
            let mut ah_attr = AddressHandleAttribute::new();

            ah_attr
                .setup_dest_lid(1)
                .setup_port(1)
                .setup_service_level(42)
                .setup_grh_src_gid_index(0)
                .setup_grh_dest_gid(&ctx.query_gid(1, 0).unwrap())
                .setup_grh_hop_limit(255);
            attr.setup_address_vector(&ah_attr);
            qp.modify(&attr)?;

            let mut attr = QueuePairAttribute::new();
            attr.setup_state(QueuePairState::ReadyToSend)
                .setup_sq_psn(utils::random::generate_psn())
                .setup_timeout(12)
                .setup_retry_cnt(7)
                .setup_rnr_retry(7)
                .setup_max_read_atomic(0);
            qp.modify(&attr)?;

            let mut cur_iter: u32 = 0;
            let mut inflight = 0;

            let clock = quanta::Clock::new();
            let start_time = clock.now();

            while cur_iter < iters {
                while inflight < tx_depth {
                    let mut guard = qp.start_post_send();

                    let offset = (cur_iter % tx_depth) as usize * msg_size as usize;
                    let addr = mr.get_ptr() as u64 + offset as u64;

                    let send_handle = guard
                        .construct_wr(cur_iter as _, WorkRequestFlags::Signaled)
                        .setup_write(mr.rkey(), addr);

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
                                panic!(
                                    "Failed status {:#?} ({}) for wr_id {}",
                                    Into::<WorkCompletionStatus>::into(wc.status()),
                                    wc.status(),
                                    wc.wr_id()
                                );
                            }
                            inflight -= 1;
                        }
                    }
                    Err(_) => continue
                }
            }

            let end_time = clock.now();
            let time = end_time.duration_since(start_time);
            let bytes = mr.region_len() as u64 * cur_iter as u64;
            let bytes_per_second = bytes as f64 / time.as_secs_f64();

            let results = BandwidthResult {
                size: msg_size,
                iterations: cur_iter,
                bandwidth: Byte::from_f64(bytes_per_second).unwrap().get_appropriate_unit(UnitType::Binary).get_value(),
                msg_rate: (cur_iter as f64 / time.as_secs_f64() / 1_000_000.0),
                time: format!("{:.2}", time.as_secs_f64()),
            };
            display.set_results(results);

            display.display();
            Ok(())
        }
        Commands::Probe(_args) => {
            let device_list = DeviceList::new().unwrap();
            // let mut devices = Vec::new();
            for device in &device_list {
                let context = device.open()?;
                let attr = context.query_device()?;

                let info = utils::device::DeviceInfo::from_device_attr(&attr);

                println!("{}: {}", context.name(), info.description());

                for i in 1..=attr.phys_port_cnt() {
                    let port_attr = context.query_port(i).unwrap();

                    println!(
                        "    Port {i}: {:?} ({} Gbps), {:?}, total data rate: {} Gbps",
                        port_attr.active_speed(),
                        port_attr.active_speed().to_throughput(),
                        port_attr.active_width(),
                        port_attr.active_speed().to_throughput()
                            * ((port_attr.active_width() as u32) as f64)
                    );
                }
            }

            Ok(())
        }
        _ => Ok(()),
    }
}
