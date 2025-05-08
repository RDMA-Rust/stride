use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "stride-perf")]
#[command(about = "RDMA performance testing tool")]
pub struct PerfCli {
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
    Bandwidth(SendBandwidthArgs),
    #[command(alias = "lat")]
    Latency(SendLatencyArgs),
}

#[derive(Args)]
pub struct SendBandwidthArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Size of Tx queue
    #[arg(long, short = 't', default_value_t = 128)]
    pub tx_depth: u32,
    /// Size of Rx queue
    #[arg(long)]
    pub rx_depth: Option<u32>,
    /// Use send-with-immediate verb instead of send
    #[arg(long)]
    pub imm_data: bool,
}

#[derive(Args)]
pub struct SendLatencyArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Size of Tx queue
    #[arg(long)]
    pub tx_depth: Option<u32>,
    /// Size of Rx queue
    #[arg(long)]
    pub rx_depth: Option<u32>,
    /// Use send-with-immediate verb instead of send
    #[arg(long)]
    pub imm_data: bool,
}

#[derive(Subcommand)]
pub enum WriteCommands {
    #[command(alias = "bw")]
    Bandwidth(WriteBandwidthArgs),
    #[command(alias = "lat")]
    Latency(WriteLatencyArgs),
}

#[derive(Args)]
pub struct WriteBandwidthArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Size of Tx queue
    #[arg(long, short = 't', default_value_t = 128)]
    pub tx_depth: u32,
    /// Use write-with-immediate verb instead of write
    #[arg(long)]
    pub imm_data: bool,
}

#[derive(Args)]
pub struct WriteLatencyArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Size of Tx queue
    #[arg(long, short = 't')]
    pub tx_depth: Option<u32>,
    /// Use write-with-immediate verb instead of write
    #[arg(long)]
    pub imm_data: bool,
}

#[derive(Subcommand)]
pub enum ReadCommands {
    #[command(alias = "bw")]
    Bandwidth(ReadBandwidthArgs),
    #[command(alias = "lat")]
    Latency(ReadLatencyArgs),
}

#[derive(Args)]
pub struct ReadBandwidthArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[arg(long)]
    pub tx_depth: Option<u32>,
}

#[derive(Args)]
pub struct ReadLatencyArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[arg(long)]
    pub tx_depth: Option<u32>,
}

#[derive(Args)]
pub struct CommonArgs {
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
    /// Post list of send WQEs of <list size> size (instead of single post)
    #[arg(long, short = 'l', default_value_t = 1)]
    pub post_list: u32,
    /// Completion queue entry poll batch size
    #[arg(long, default_value_t = 32)]
    pub cqe_poll: u32,
    /// Use hugepages for memory allocations
    #[arg(long)]
    pub use_hugepages: bool,
    /// Use flow control to prevent sender from overwhelming receiver
    #[arg(long)]
    pub use_flow_control: bool,
}
