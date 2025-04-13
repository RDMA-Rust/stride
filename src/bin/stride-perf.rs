use clap::Parser;
use stride::cli::perf::{PerfCli, PerfCommands, ReadCommands, SendCommands, WriteCommands};
use stride::runners::runner::TestRunner;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = PerfCli::parse();

    match cli.command {
        PerfCommands::Send(cmd) => match cmd {
            SendCommands::Bandwidth(args) => {
                let params = stride::cli::params::SendBandwidthParams::from_args(&args);
                let runner = TestRunner::new(params);
                runner.run()?;
            }
            SendCommands::Latency(args) => {
                let params = stride::cli::params::SendLatencyParams::from_args(&args);
                let runner = TestRunner::new(params);
                runner.run()?;
            }
        },
        PerfCommands::Write(cmd) => match cmd {
            WriteCommands::Bandwidth(args) => {
                let params = stride::cli::params::WriteBandwidthParams::from_args(&args);
                let runner = TestRunner::new(params);
                runner.run()?;
            }
            WriteCommands::Latency(args) => {
                let params = stride::cli::params::WriteLatencyParams::from_args(&args);
                let runner = TestRunner::new(params);
                runner.run()?;
            }
        },
        PerfCommands::Read(cmd) => match cmd {
            ReadCommands::Bandwidth(args) => {
                let params = stride::cli::params::ReadBandwidthParams::from_args(&args);
                let runner = TestRunner::new(params);
                runner.run()?;
            }
            ReadCommands::Latency(args) => {
                let params = stride::cli::params::ReadLatencyParams::from_args(&args);
                let runner = TestRunner::new(params);
                runner.run()?;
            }
        },
    }

    Ok(())
}
