use clap::{Parser};

#[derive(Parser)]
#[command(name = "stride-probe")]
#[command(about = "RDMA device probing tool")]
pub struct ProbeCli {
    /// Show detailed device information
    #[arg(long, short = 'd')]
    pub detailed: bool,

    /// Show NUMA information
    #[arg(long)]
    pub numa: bool,

    /// Show only specific device
    #[arg(long)]
    pub device: Option<String>,
}
