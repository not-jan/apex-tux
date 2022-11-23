use anyhow::Result;
use apex_hardware::{Device, USBDevice};
use clap::{ArgAction, Parser, Subcommand};
use log::{info, LevelFilter};
use simplelog::{Config as LoggerConfig, SimpleLogger};

#[derive(Parser)]
#[clap(version = "1.0", author = "not-jan")]
struct Opts {
    /// A level of verbosity, and can be used multiple times
    #[arg(short, long, action = ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    subcmd: SubCommand,
}

#[derive(Subcommand)]
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
