use crate::app::{get_list_of_files, App};
use crate::processor::Filter;
use crate::slim_server_connector::build_slim_server_connector;
use anyhow::{anyhow, bail, Result};
use clap::Parser;
use std::{fs::read_to_string, path::PathBuf};
use toml::Table;

mod app;
mod port;
mod processor;
mod slim_server_connector;

/// Test markdown files using a slim server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Configuration file
    #[arg(short, long)]
    configuration_file: Option<PathBuf>,
    /// Base port to connect to the slim server. Default is 8085
    #[arg(short, long)]
    port: Option<u16>,
    /// The size of the pool of ports to cycle through. Default is 20 (8085 - 8105)
    #[arg(short = 'l', long)]
    pool_size: Option<u16>,
    /// Command to start the slim server
    #[arg(short = 'x', long)]
    execute_server_command: Option<String>,
    /// Recursively traverse files and directories to test
    #[arg(short, long)]
    recursive: bool,
    /// Show snoozed errors
    #[arg(short, long)]
    show_snoozed: bool,
    /// Pipe STDERR and STDOUT of the slim server through the STDOUT
    #[arg(short = 'o', long)]
    pipe_output: bool,
    /// Extension to filter markdown files. Default is md. Not case-sensitive
    #[arg(short, long)]
    extension: Option<String>,
    /// Filter the decision tables by the fixture class. Accept any regex string
    #[arg(short = 'f', long)]
    class_filter: Option<String>,
    /// List of files to test
    files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = append_config_to_args(Args::parse())?;
    let Some(command) = args.execute_server_command else {
        bail!("You need to provide a command to start the slim server")
    };

    let mut filter = Filter::new();
    if let Some(fixture) = args.class_filter {
        filter = filter.fixture_class(&fixture)?;
    }

    if App::new(
        args.show_snoozed,
        build_slim_server_connector(
            command,
            args.port.unwrap_or(8085),
            args.pool_size.unwrap_or(20),
            args.pipe_output,
        ),
        args.recursive,
        filter,
        args.extension.unwrap_or("md".to_string()).to_lowercase(),
        args.files,
    )
    .run()?
    {
        bail!("Tests executed with error");
    }

    Ok(())
}

fn config_u16(config_file: &Table, key: &str) -> Result<Option<u16>> {
    config_file
        .get(key)
        .map(|value| {
            let value = value
                .as_integer()
                .ok_or_else(|| anyhow!("Expected `{key}` to be a number"))?;
            u16::try_from(value)
                .map_err(|_| anyhow!("Expected `{key}` to be between 0 and 65535 but got {value}"))
        })
        .transpose()
}

fn append_config_to_args(mut args: Args) -> Result<Args> {
    match &args.configuration_file {
        None => Ok(args),
        Some(configuration_file) => {
            let config_file = read_to_string(configuration_file)?.parse::<Table>()?;
            args.port = args.port.or(config_u16(&config_file, "port")?);
            args.pool_size = args.pool_size.or(config_u16(&config_file, "pool_size")?);
            args.recursive = args.recursive
                || config_file.get("recursive").is_some_and(|recursive| {
                    recursive
                        .as_bool()
                        .expect("Expect the recursive to be a boolean")
                });
            args.show_snoozed = args.show_snoozed
                || config_file.get("show_snoozed").is_some_and(|show_snoozed| {
                    show_snoozed
                        .as_bool()
                        .expect("Expect the show_snoozed to be a boolean")
                });
            args.pipe_output = args.pipe_output
                || config_file.get("pipe_output").is_some_and(|verbose| {
                    verbose
                        .as_bool()
                        .expect("Expect the verbose to be a boolean")
                });
            args.execute_server_command = args.execute_server_command.or(config_file
                .get("execute_server_command")
                .map(|command| {
                    command
                        .as_str()
                        .expect("Expect the slim server command to be a string")
                        .to_string()
                }));
            args.extension = args
                .extension
                .or(config_file.get("extension").map(|command| {
                    command
                        .as_str()
                        .expect("Expect the slim server command to be a string")
                        .to_string()
                }));
            args.files = if args.files.is_empty() {
                let test_dir = config_file
                    .get("test_dir")
                    .map(|test_dir| {
                        test_dir
                            .as_str()
                            .expect("Expect the test dir to be a string")
                    })
                    .ok_or(anyhow!("You need to specify a test file or a test dir"))?;
                get_list_of_files(test_dir)?
            } else {
                args.files
            };
            Ok(args)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn config(toml: &str) -> Table {
        toml.parse().unwrap()
    }

    #[test]
    fn config_u16_reads_valid_values() {
        let config = config("port = 8085");
        assert_eq!(Some(8085), config_u16(&config, "port").unwrap());
        assert_eq!(None, config_u16(&config, "pool_size").unwrap());
    }

    #[test]
    fn config_u16_rejects_out_of_range_values() {
        for value in ["70000", "65537", "-1"] {
            let config = config(&format!("port = {value}"));
            let error = config_u16(&config, "port").unwrap_err();
            assert_eq!(
                format!("Expected `port` to be between 0 and 65535 but got {value}"),
                error.to_string()
            );
        }
    }

    #[test]
    fn config_u16_rejects_values_that_are_not_numbers() {
        let error = config_u16(&config("pool_size = \"ten\""), "pool_size").unwrap_err();
        assert_eq!("Expected `pool_size` to be a number", error.to_string());
    }
}
