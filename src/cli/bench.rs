use clap::Parser;

#[derive(Parser)]
#[command(name = "stride-bench")]
#[command(about = "RDMA benchmarking tool")]
pub struct BenchCli {
    /// Enable NUMA-aware benchmarking
    #[arg(long)]
    pub numa: bool,

    /// Benchmark duration in seconds
    #[arg(long, short = 't', default_value_t = 10)]
    pub duration: u32,

    /// Message sizes to benchmark (comma separated list or range)
    #[arg(long, short = 's', default_value = "1024,4096,16384,65536")]
    pub sizes: String,
}
