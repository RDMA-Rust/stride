use clap::Parser;
use stride::cli::perf::Cli;
use stride::cli::plan::execute;
use tracing_subscriber::filter::LevelFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    // Configure tracing based on CLI flags
    if cli.trace {
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::TRACE)
            .init();
    } else {
        // Default: only show errors
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::ERROR)
            .init();
    }

    // Convert CLI to Plan and execute
    let plan = cli.try_into()?;
    execute(plan)?;

    Ok(())
}
