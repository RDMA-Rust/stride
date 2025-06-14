use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::connection::exchange::MemoryRegionInfo;

#[derive(Parser)]
#[command(name = "stride-perf", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub op: OpCmd,
}

#[derive(Subcommand)]
pub enum OpCmd {
    Send(SendCmd),
    Write(WriteCmd),
    Read(ReadCmd),
}

#[derive(Debug, Clone)]
pub struct PlanBase {
    pub mode: Mode,
    pub dev: Option<String>,
    pub gid_index: Option<u8>,
    pub server: bool,
    pub addr: String,
    pub threads: usize,
    pub msg_sizes: Vec<u32>,
    pub iters: u32,
    pub bidir: bool,
    pub tx_depth: u32,
    pub timeout: u8,
    pub post_list: u32,
    pub cqe_poll: u32,
    pub hugepages: bool,
}

#[derive(Debug, Clone)]
pub enum Plan {
    Send(SendPlan),
    Write(WritePlan),
    Read(ReadPlan),
}

#[derive(Debug, Clone)]
pub struct SendPlan {
    pub base: PlanBase,
    pub rx_depth: u32,
    pub imm_data: bool,
    pub flow_control: bool,
}

#[derive(Debug, Clone)]
pub struct WritePlan {
    pub base: PlanBase,
    pub imm_data: bool,
    pub remote_mr: Option<MemoryRegionInfo>,
}

#[derive(Debug, Clone)]
pub struct ReadPlan {
    pub base: PlanBase,
    pub remote_mr: Option<MemoryRegionInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Send,
    Write,
    Read,
    Atomic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Bandwidth,
    Latency,
}

impl Plan {
    pub fn base(&self) -> &PlanBase {
        match self {
            Plan::Send(p) => &p.base,
            Plan::Write(p) => &p.base,
            Plan::Read(p) => &p.base,
        }
    }

    pub fn operation(&self) -> Operation {
        match self {
            Plan::Send(_) => Operation::Send,
            Plan::Write(_) => Operation::Write,
            Plan::Read(_) => Operation::Read,
        }
    }

    pub fn poll_batch(&self) -> u32 {
        self.base().cqe_poll
    }

    pub fn is_latency(&self) -> bool {
        self.base().mode == Mode::Latency
    }

    pub fn needs_remote_addr(&self) -> bool {
        matches!(self.operation(), Operation::Write | Operation::Read)
    }

    pub fn test_name(&self) -> &'static str {
        match (self.operation(), self.base().mode) {
            (Operation::Send, Mode::Bandwidth) => "SEND bw",
            (Operation::Send, Mode::Latency) => "SEND lat",
            (Operation::Write, Mode::Bandwidth) => "WRITE bw",
            (Operation::Write, Mode::Latency) => "WRITE lat",
            (Operation::Read, Mode::Bandwidth) => "READ bw",
            (Operation::Read, Mode::Latency) => "READ lat",
            (Operation::Atomic, Mode::Bandwidth) => "ATOMIC bw",
            (Operation::Atomic, Mode::Latency) => "ATOMIC lat",
        }
    }

    pub fn rx_depth(&self) -> Option<u32> {
        match self {
            Plan::Send(p) => Some(p.rx_depth),
            _ => None, // WRITE and READ don't need rx_depth
        }
    }

    pub fn uses_immediate_data(&self) -> bool {
        match self {
            Plan::Send(p) => p.imm_data,
            Plan::Write(p) => p.imm_data,
            Plan::Read(_) => false, // READ doesn't support immediate data
        }
    }

    pub fn uses_flow_control(&self) -> bool {
        match self {
            Plan::Send(p) => p.flow_control,
            _ => false, // Only SEND supports flow control
        }
    }

    /// Get remote memory region (for one-sided operations)
    pub fn remote_mr(&self) -> Option<&MemoryRegionInfo> {
        match self {
            Plan::Send(_) => None, // SEND doesn't use remote MR
            Plan::Write(p) => p.remote_mr.as_ref(),
            Plan::Read(p) => p.remote_mr.as_ref(),
        }
    }

    /// Set remote memory region (called during connection setup)
    pub fn set_remote_mr(&mut self, remote_mr: MemoryRegionInfo) {
        match self {
            Plan::Send(_) => {}, // SEND doesn't use remote MR
            Plan::Write(p) => p.remote_mr = Some(remote_mr),
            Plan::Read(p) => p.remote_mr = Some(remote_mr),
        }
    }
}

// Common options shared by all operations
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
    /// Size of Tx queue
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
}

// SEND-specific command structure
#[derive(Args)]
pub struct SendCmd {
    #[command(subcommand)]
    pub mode: SendMode,
}

#[derive(Subcommand)]
pub enum SendMode {
    #[command(alias = "bw")]
    Bandwidth(SendOpts),
    #[command(alias = "lat")]
    Latency(SendOpts),
}

#[derive(Args, Debug)]
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

// WRITE-specific command structure
#[derive(Args)]
pub struct WriteCmd {
    #[command(subcommand)]
    pub mode: WriteMode,
}

#[derive(Subcommand)]
pub enum WriteMode {
    #[command(alias = "bw")]
    Bandwidth(WriteOpts),
    #[command(alias = "lat")]
    Latency(WriteOpts),
}

#[derive(Args, Debug)]
pub struct WriteOpts {
    #[command(flatten)]
    pub common: CommonOpts,
    /// Use write-with-immediate verb instead of write
    #[arg(long)]
    pub imm_data: bool,
}

// READ-specific command structure
#[derive(Args)]
pub struct ReadCmd {
    #[command(subcommand)]
    pub mode: ReadMode,
}

#[derive(Subcommand)]
pub enum ReadMode {
    #[command(alias = "bw")]
    Bandwidth(ReadOpts),
    #[command(alias = "lat")]
    Latency(ReadOpts),
}

#[derive(Args, Debug)]
pub struct ReadOpts {
    #[command(flatten)]
    pub common: CommonOpts,
    // No additional options - READ is the simplest operation
}

impl TryFrom<Cli> for Plan {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        let (op, mode, common, send_opts, write_opts, read_opts) = match cli.op {
            OpCmd::Send(SendCmd {
                mode: SendMode::Bandwidth(opts),
            }) => (
                Operation::Send,
                Mode::Bandwidth,
                opts.common,
                Some((opts.rx_depth, opts.imm_data, opts.use_flow_control)),
                None,
                None,
            ),
            OpCmd::Send(SendCmd {
                mode: SendMode::Latency(opts),
            }) => (
                Operation::Send,
                Mode::Latency,
                opts.common,
                Some((opts.rx_depth, opts.imm_data, opts.use_flow_control)),
                None,
                None,
            ),
            OpCmd::Write(WriteCmd {
                mode: WriteMode::Bandwidth(opts),
            }) => (
                Operation::Write,
                Mode::Bandwidth,
                opts.common,
                None,
                Some(opts.imm_data),
                None,
            ),
            OpCmd::Write(WriteCmd {
                mode: WriteMode::Latency(opts),
            }) => (
                Operation::Write,
                Mode::Latency,
                opts.common,
                None,
                Some(opts.imm_data),
                None,
            ),
            OpCmd::Read(ReadCmd {
                mode: ReadMode::Bandwidth(opts),
            }) => (
                Operation::Read,
                Mode::Bandwidth,
                opts.common,
                None,
                None,
                Some(()),
            ),
            OpCmd::Read(ReadCmd {
                mode: ReadMode::Latency(opts),
            }) => (
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
                let imm_data = write_opts.unwrap();
                Plan::Write(WritePlan {
                    base,
                    imm_data,
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

/// Execute a benchmark plan using the new worker architecture
pub fn execute(plan: Plan) -> anyhow::Result<()> {
    use crate::runners::worker::run_worker;

    run_worker(plan)
}
