use clap::Parser;
use std::process;

use klinker::{run, CLIConfig};

fn main() {
    let config = CLIConfig::parse();

    if let Err(e) = run(&config) {
        eprintln!("{}", e);

        process::exit(1);
    }
}
