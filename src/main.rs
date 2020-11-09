use clap::{App, Arg};
use std::process;

use klinker::{run, CLIConfig};

fn main() {
    let matches = App::new("Kerbal Assembler")
        .version(klinker::VERSION)
        .author("Luke Newcomb")
        .about("Links KerbalObject files into KSM files to be run by kOS.")
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input file to use")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("output_path")
                .help("Sets the output file to use")
                .short("o")
                .long("output")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("debug")
                .help("Displays debugging information during the assembly process.")
                .short("d")
                .long("debug"),
        )
        .get_matches();

    let config = CLIConfig::new(matches);

    if let Err(e) = run(&config) {
        eprintln!("Application error: {}", e);

        process::exit(1);
    }
}
