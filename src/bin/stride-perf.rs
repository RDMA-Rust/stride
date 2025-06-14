use clap::Parser;
use stride::cli::plan::{Cli, execute};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    tracing_subscriber::fmt::init();

    // Convert CLI to Plan and execute
    let plan = cli.try_into()?;
    execute(plan)?;

    Ok(())
}
