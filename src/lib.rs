use clap::ArgMatches;
use std::error::Error;

pub static VERSION: &'static str = "0.1.0";

pub fn run(config: &CLIConfig) -> Result<(), Box<dyn Error>> {

    let mut output_path = config.output_path_value.clone();

        // If the output path was not specified
        if output_path.is_empty() {
            // Create a new string the same as file_path
            output_path = config.file_path.clone();
    
            // Replace the file extension of .ko with .ksm
            output_path.replace_range((output_path.len() - 2).., "ksm");
        } else if !output_path.ends_with(".ksm") {
            output_path.push_str(".ksm");
        }
    
        if config.debug {
            println!("Outputting to: {}", output_path);
        }
    
        Ok(())
}

pub struct CLIConfig {
    pub file_path: String,
    pub output_path_value: String,
    pub debug: bool,
}

impl CLIConfig {
    pub fn new(matches: ArgMatches) -> CLIConfig {
        CLIConfig {
            file_path: String::from(matches.value_of("INPUT").unwrap()),
            output_path_value: if matches.is_present("output_path") {
                String::from(matches.value_of("output_path").unwrap())
            } else {
                String::new()
            },
            debug: matches.is_present("debug"),
        }
    }
}
