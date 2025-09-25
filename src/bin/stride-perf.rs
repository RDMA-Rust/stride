use clap::Parser;
use stride::cli::perf::Cli;
use stride::cli::plan::{execute, Plan};
use tracing_subscriber::filter::LevelFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Convert CLI to Plan first to get merged trace setting
    let plan: Plan = cli.try_into()?;

    // Configure tracing based on merged trace setting
    if plan.base().output.trace_enabled {
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::TRACE)
            .init();
    } else {
        // Default: only show errors
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::ERROR)
            .init();
    }

    execute(plan)?;

    Ok(())
}
