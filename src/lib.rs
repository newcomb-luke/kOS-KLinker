use clap::ArgMatches;
use driver::Driver;
use std::error::Error;
use std::io::prelude::*;

pub mod driver;

pub mod tables;

use kerbalobjects::ToBytes;

pub static VERSION: &'static str = env!("CARGO_PKG_VERSION");

pub fn run(config: &CLIConfig) -> Result<(), Box<dyn Error>> {
    let mut output_path = config.output_path_value.clone();

    if !output_path.ends_with(".ksm") {
        output_path.push_str(".ksm");
    }

    let mut driver = Driver::new(config.to_owned());

    for file_path in &config.file_paths {
        driver.add(file_path);
    }

    let ksm_file = driver.link()?;

    let mut file_buffer = Vec::with_capacity(2048);

    ksm_file.to_bytes(&mut file_buffer);

    let mut file = std::fs::File::create(output_path)?;

    file.write_all(file_buffer.as_slice())?;

    Ok(())
}

#[derive(Debug, Clone)]
pub struct CLIConfig {
    pub file_paths: Vec<String>,
    pub output_path_value: String,
    pub entry_point: String,
    pub shared: bool,
    pub debug: bool,
}

impl CLIConfig {
    pub fn new(matches: ArgMatches) -> CLIConfig {
        CLIConfig {
            file_paths: {
                let mut v = Vec::new();

                for s in matches.values_of("INPUT").unwrap() {
                    v.push(String::from(s));
                }

                v
            },
            output_path_value: String::from(matches.value_of("output_path").unwrap()),
            entry_point: String::from(matches.value_of("entry_point").unwrap()),
            shared: matches.is_present("shared_object"),
            debug: matches.is_present("debug"),
        }
    }
}
