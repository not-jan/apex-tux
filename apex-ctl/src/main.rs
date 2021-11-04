use anyhow::Result;
use apex_hardware::{Device, FrameBuffer, USBDevice};
use clap::Parser;
use log::{info, LevelFilter};
use simplelog::{Config as LoggerConfig, SimpleLogger};

#[derive(Parser)]
#[clap(version = "1.0", author = "not-jan")]
struct Opts {
    /// A level of verbosity, and can be used multiple times
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    /// Clear the OLED screen
    Clear,
    /// Fill the OLED screen
    Fill,
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    let filter = match opts.verbose {
        0 => LevelFilter::Info,
        1 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    SimpleLogger::init(filter, LoggerConfig::default())?;

    info!("Connecting to the USB device");

    let mut device = USBDevice::try_connect()?;

    match opts.subcmd {
        SubCommand::Clear => device.clear()?,
        SubCommand::Fill => device.fill()?,
    };

    Ok(())
}
