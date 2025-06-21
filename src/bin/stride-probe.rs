use clap::Parser;
use stride::cli::probe::ProbeCli;
use stride::utils::device::probe_devices;
use tracing_subscriber::filter::LevelFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = ProbeCli::parse();

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

    // Determine TUI setting using same logic as stride-perf
    let tui_enabled = if cli.trace && !cli.tui {
        false // Trace enabled, TUI not explicitly enabled -> disable TUI
    } else {
        true // Default case or both flags provided
    };

    probe_devices(cli.detailed, cli.numa, cli.device.as_deref(), tui_enabled)?;

    Ok(())
}
