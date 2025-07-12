use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use sideway::ibverbs::device_context::Mtu;

use crate::cli::plan::{
    Mode, Operation, OutputConfig, Plan, PlanBase, ReadPlan, SendPlan, WritePlan,
};

#[derive(Clone, Copy, Debug)]
pub struct PathMtu(pub Mtu);

impl ValueEnum for PathMtu {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self(Mtu::Mtu256),
            Self(Mtu::Mtu512),
            Self(Mtu::Mtu1024),
            Self(Mtu::Mtu2048),
            Self(Mtu::Mtu4096),
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self.0 {
            Mtu::Mtu256 => Some(clap::builder::PossibleValue::new("256")),
            Mtu::Mtu512 => Some(clap::builder::PossibleValue::new("512")),
            Mtu::Mtu1024 => Some(clap::builder::PossibleValue::new("1024")),
            Mtu::Mtu2048 => Some(clap::builder::PossibleValue::new("2048")),
            Mtu::Mtu4096 => Some(clap::builder::PossibleValue::new("4096")),
        }
    }
}

impl Default for PathMtu {
    fn default() -> Self {
        Self(Mtu::Mtu4096)
    }
}

#[derive(Parser)]
#[command(name = "stride-perf")]
#[command(about = "RDMA performance testing tool")]
pub struct Cli {
    /// Enable trace-level logging output
    #[arg(long)]
    pub trace: bool,
    /// Enable TUI (terminal user interface) output (enabled by default)
    #[arg(long)]
    pub tui: bool,
    #[command(subcommand)]
    pub command: PerfCommands,
}

#[derive(Subcommand)]
pub enum PerfCommands {
    #[command(subcommand)]
    Send(SendCommands),
    #[command(subcommand)]
    Write(WriteCommands),
    #[command(subcommand)]
    Read(ReadCommands),
}

#[derive(Subcommand)]
pub enum SendCommands {
    #[command(alias = "bw")]
    Bandwidth(SendOpts),
    #[command(alias = "lat")]
    Latency(SendOpts),
}

#[derive(Args)]
pub struct SendOpts {
    #[command(flatten)]
    pub common: CommonOpts,
    /// Size of Rx queue
    #[arg(long, default_value_t = 512)]
    pub rx_depth: u32,
    /// Use send-with-immediate verb instead of send
    #[arg(long)]
    pub imm_data: bool,

    /// Use flow control to prevent sender from overwhelming receiver
    #[arg(long)]
    pub use_flow_control: bool,
}

#[derive(Subcommand)]
pub enum WriteCommands {
    #[command(alias = "bw")]
    Bandwidth(WriteOpts),
    #[command(alias = "lat")]
    Latency(WriteOpts),
}

#[derive(Args)]
pub struct WriteOpts {
    #[command(flatten)]
    pub common: CommonOpts,
    /// Use write-with-immediate verb instead of write
    #[arg(long)]
    pub imm_data: bool,
    /// Size of Rx queue (only valid with --imm-data)
    #[arg(long, default_value_t = 512)]
    pub rx_depth: u32,
}

#[derive(Subcommand)]
pub enum ReadCommands {
    #[command(alias = "bw")]
    Bandwidth(ReadOpts),
    #[command(alias = "lat")]
    Latency(ReadOpts),
}

#[derive(Args)]
pub struct ReadOpts {
    #[command(flatten)]
    pub common: CommonOpts,
}

#[derive(Args, Debug)]
pub struct CommonOpts {
    /// Use IB device <DEVICE> [default: first device found]
    #[arg(long, short = 'd')]
    pub device: Option<String>,
    /// Test uses GID with GID index taken from command
    #[arg(long, short = 'x')]
    pub gid_index: Option<u8>,
    /// Number of exchanges (at least 100)
    #[arg(long, short = 'n', default_value_t = 1000)]
    pub iters: u32,
    /// Message size in bytes
    #[arg(long, short = 's', default_value_t = 65536)]
    pub msg_size: u32,
    /// QP timeout = (4 us) * (2 ^ timeout)
    #[arg(long, short = 'u', default_value_t = 14)]
    pub qp_timeout: u8,
    /// Port number for connections
    #[arg(long, short = 'p', default_value_t = 18515)]
    pub port: u16,
    /// Server address (if specified, run as client connecting to this server)
    #[arg(index = 1)]
    pub server_address: Option<String>,
    /// Number of queue pairs to use
    #[arg(long, short = 'q', default_value_t = 1)]
    pub qp_count: u32,
    /// Use bidirectional traffic pattern instead of default unidirectional
    #[arg(long, short = 'b', default_value_t = false)]
    pub bidirectional: bool,
    /// Run test with all message sizes (2 bytes to 32 MiB)
    #[arg(long, short = 'a')]
    pub all_sizes: bool,
    /// Multiplier between message sizes when using --all-sizes
    #[arg(long, default_value_t = 2.0)]
    pub step_factor: f64,
    /// Addition to next message size when using --all-sizes
    #[arg(long, default_value_t = 0)]
    pub step_addition: u32,
    /// Maximum message size (bytes) when using --all-sizes
    #[arg(long, default_value_t = 33_554_432)]
    pub max_msg_size: u32,
    /// Size of Tx queue (would be 1 for latency tests)
    #[arg(long, short = 't', default_value_t = 128)]
    pub tx_depth: u32,
    /// Post list of send WQEs of <list size> size (instead of single post)
    #[arg(long, short = 'l', default_value_t = 1)]
    pub post_list: u32,
    /// Completion queue entry poll batch size
    #[arg(long, default_value_t = 32)]
    pub cqe_poll: u32,
    /// Use hugepages for memory allocations
    #[arg(long)]
    pub use_hugepages: bool,
    /// MTU size (Byte)
    #[arg(long, short = 'm', value_enum, default_value_t = PathMtu::default())]
    pub mtu: PathMtu,
}

impl TryFrom<Cli> for Plan {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        let (op, mode, common, send_opts, write_opts, read_opts) = match cli.command {
            PerfCommands::Send(SendCommands::Bandwidth(opts)) => (
                Operation::Send,
                Mode::Bandwidth,
                opts.common,
                Some((opts.rx_depth, opts.imm_data, opts.use_flow_control)),
                None,
                None,
            ),
            PerfCommands::Send(SendCommands::Latency(opts)) => (
                Operation::Send,
                Mode::Latency,
                opts.common,
                Some((opts.rx_depth, opts.imm_data, opts.use_flow_control)),
                None,
                None,
            ),
            PerfCommands::Write(WriteCommands::Bandwidth(opts)) => {
                // Validate rx_depth usage
                if opts.rx_depth != 512 && !opts.imm_data {
                    return Err(anyhow::anyhow!(
                        "Error: --rx-depth can only be used with --imm-data for write operations.\n\
                         Regular write operations don't use receive queues.\n\
                         Use: stride-perf write bw --imm-data --rx-depth {}", opts.rx_depth
                    ));
                }
                (
                    Operation::Write,
                    Mode::Bandwidth,
                    opts.common,
                    None,
                    Some((opts.imm_data, opts.rx_depth)),
                    None,
                )
            }
            PerfCommands::Write(WriteCommands::Latency(opts)) => {
                // Validate rx_depth usage
                if opts.rx_depth != 512 && !opts.imm_data {
                    return Err(anyhow::anyhow!(
                        "Error: --rx-depth can only be used with --imm-data for write operations.\n\
                         Regular write operations don't use receive queues.\n\
                         Use: stride-perf write lat --imm-data --rx-depth {}", opts.rx_depth
                    ));
                }
                (
                    Operation::Write,
                    Mode::Latency,
                    opts.common,
                    None,
                    Some((opts.imm_data, opts.rx_depth)),
                    None,
                )
            }
            PerfCommands::Read(ReadCommands::Bandwidth(opts)) => (
                Operation::Read,
                Mode::Bandwidth,
                opts.common,
                None,
                None,
                Some(()),
            ),
            PerfCommands::Read(ReadCommands::Latency(opts)) => (
                Operation::Read,
                Mode::Latency,
                opts.common,
                None,
                None,
                Some(()),
            ),
        };

        /* expand sizes once */
        let msg_sizes = if common.all_sizes {
            let mut v = vec![2];
            let mut size = 2;
            while size < common.max_msg_size {
                size = ((size as f64 * common.step_factor).ceil() as u32) + common.step_addition;
                v.push(size.min(common.max_msg_size));
            }
            v
        } else {
            vec![common.msg_size]
        };

        // Determine server mode and address
        let (server, addr) = if let Some(target_addr) = common.server_address {
            (false, format!("{}:{}", target_addr, common.port))
        } else {
            (true, format!("0.0.0.0:{}", common.port))
        };

        // Determine output configuration based on CLI flags
        let output = OutputConfig {
            trace_enabled: cli.trace,
            // TUI is enabled by default, but disabled when trace is enabled unless both are explicitly provided
            tui_enabled: if cli.trace && !cli.tui {
                false // Trace enabled, TUI not explicitly enabled -> disable TUI
            } else {
                true // Default case or both flags provided
            },
        };

        // Create common base
        let base = PlanBase {
            mode,
            dev: common.device,
            gid_index: common.gid_index,
            server,
            addr,
            threads: common.qp_count as usize,
            msg_sizes,
            iters: common.iters,
            bidir: common.bidirectional,
            /* verbs */
            tx_depth: if mode == Mode::Latency {
                1
            } else {
                common.tx_depth
            },
            timeout: common.qp_timeout,
            post_list: common.post_list,
            cqe_poll: common.cqe_poll,
            hugepages: common.use_hugepages,
            mtu: common.mtu.0,
            output,
        };

        // Create operation-specific plan
        let plan = match op {
            Operation::Send => {
                let (rx_depth, imm_data, use_flow_control) = send_opts.unwrap();
                Plan::Send(SendPlan {
                    base,
                    rx_depth,
                    imm_data,
                    flow_control: use_flow_control,
                })
            }
            Operation::Write => {
                let (imm_data, rx_depth) = write_opts.unwrap();
                Plan::Write(WritePlan {
                    base,
                    imm_data,
                    rx_depth,
                    remote_mr: None, // Set during connection setup
                })
            }
            Operation::Read => {
                let _unit = read_opts.unwrap();
                Plan::Read(ReadPlan {
                    base,
                    remote_mr: None, // Set during connection setup
                })
            }
            Operation::Atomic => {
                return Err(anyhow::anyhow!("Atomic operations not yet implemented"));
            }
        };

        Ok(plan)
    }
}
