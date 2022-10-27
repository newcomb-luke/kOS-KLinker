use clap::Parser;
use driver::Driver;
use std::error::Error;
use std::io::prelude::*;
use std::path::PathBuf;

pub mod driver;

pub mod tables;

use kerbalobjects::ToBytes;

pub static VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(config: &CLIConfig) -> Result<(), Box<dyn Error>> {
    let mut output_path = config.output_path.clone();

    if output_path.extension().is_none() {
        output_path.set_extension(".ksm");
    }

    let mut driver = Driver::new(config.to_owned());

    for file_path in &config.input_paths {
        driver.add(file_path);
    }

    let ksm_file = driver.link()?;

    let mut file_buffer = Vec::with_capacity(2048);

    ksm_file.to_bytes(&mut file_buffer);

    let mut file = std::fs::File::create(output_path)?;

    file.write_all(file_buffer.as_slice())?;

    Ok(())
}

/// This structure controls all the settings that make this program perform differently
/// These represent command-line arguments read in by clap
#[derive(Debug, Clone, Parser)]
#[command(author, version, about, long_about = None)]
pub struct CLIConfig {
    /// All of the input file paths, at least 1 is required.
    #[arg(
        value_name = "INPUT",
        help = "Sets the input path(s) to kld",
        required = true,
        num_args = 1..
    )]
    pub input_paths: Vec<PathBuf>,
    /// The required output path. Extension optional.
    #[arg(value_name = "OUTPUT", help = "The output file path")]
    pub output_path: PathBuf,
    /// A custom entry-point for the KSM program. Defaults to _start
    #[arg(
        short = 'e',
        long = "entry-point",
        require_equals = true,
        value_name = "NAME",
        default_value = "_init",
        help = "The name of the function that the program should begin execution in"
    )]
    pub entry_point: String,
    /// If the output should be a "shared library" version of a KSM file
    #[arg(
        short = 's',
        long = "shared",
        help = "Will link the object files into a shared object file instead of being linked into an executable file"
    )]
    pub shared: bool,
    /// Outputs a log of debugging information, mostly for the developers of this tool
    #[arg(
        short = 'd',
        long = "debug",
        help = "Outputs a log of debugging information, mostly for the developers of this tool"
    )]
    pub debug: bool,
}
