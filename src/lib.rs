use clap::ArgMatches;
use kerbalobjects::*;
use std::{error::Error, fs};

mod linking;
pub use linking::*;

mod ksm;
pub use ksm::*;

pub static VERSION: &'static str = "1.0.0";

pub fn run(config: &CLIConfig) -> Result<(), Box<dyn Error>> {
    let mut output_path = config.output_path_value.clone();

    if !output_path.ends_with(".ksm") {
        output_path.push_str(".ksm");
    }

    let mut kofiles = Vec::new();

    for file_path in &config.file_paths {
        let raw_contents = fs::read(&file_path)?;

        let mut reader = KOFileReader::new(raw_contents)?;
        let kofile = KOFile::read(&mut reader)?;

        kofiles.push(kofile);
    }

    let mut ksm_file = Linker::link(kofiles, config.debug, config.shared)?;

    let mut writer = KSMFileWriter::new(&output_path);

    ksm_file.write(&mut writer)?;

    writer.write_to_file()?;

    Ok(())
}

pub struct CLIConfig {
    pub file_paths: Vec<String>,
    pub output_path_value: String,
    pub shared: bool,
    pub debug: bool
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
            output_path_value: if matches.is_present("output_path") {
                String::from(matches.value_of("output_path").unwrap())
            } else {
                String::new()
            },
            shared: matches.is_present("shared"),
            debug: matches.is_present("debug")
        }
    }
}
