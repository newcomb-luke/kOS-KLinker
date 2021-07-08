use clap::{App, Arg};
use std::process;

use klinker::{run, CLIConfig};

fn main() {
    let matches = App::new("Kerbal Linker")
        .version(klinker::VERSION)
        .author("Luke Newcomb")
        .about("Links KerbalObject files into KSM files to be run by kOS.")
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input file(s) to use")
                .required(true)
                .index(1)
                .min_values(1)
        )
        .arg(
            Arg::with_name("output_path")
                .help("Sets the output file to use")
                .short("o")
                .long("output")
                .takes_value(true)
                .required(true)
        )
        .arg(
            Arg::with_name("entry_point")
                .help("The name of the function that the program should begin execution")
                .short("e")
                .long("entry-point")
                .takes_value(true)
                .default_value("_start")
        )
        .arg(
            Arg::with_name("shared_object")
                .help("Will link the object files into a shared object file instead of being linked into an executable file")
                .short("s")
                .long("shared")
        )
        .arg(
            Arg::with_name("debug")
                .help("Outputs some debugging information that probably is only useful for developers of this tool")
                .short("d")
                .long("debug")
        )
        .get_matches();

    let config = CLIConfig::new(matches);

    if let Err(e) = run(&config) {
        eprintln!("{}", e);

        process::exit(1);
    }
}
