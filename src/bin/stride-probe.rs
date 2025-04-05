use clap::Parser;
use stride::cli::probe::ProbeCli;
use stride::utils::device::probe_devices;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = ProbeCli::parse();

    probe_devices(cli.detailed, cli.numa, cli.device.as_deref())?;

    Ok(())
}
