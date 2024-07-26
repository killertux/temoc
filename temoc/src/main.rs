use crate::app::{get_list_of_files, App};
use crate::processor::Filter;
use anyhow::{anyhow, bail, Result};
use clap::Parser;
use std::{fs::read_to_string, path::PathBuf};
use toml::Table;

mod app;
mod processor;

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
    /// The size of the pool of ports to cycle through. Default is 10 (8085 - 8095)
    #[arg(short = 'l', long)]
    pool_size: Option<u8>,
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
    #[arg(short, long)]
    filter_fixture: Option<String>,
    /// List of files to test
    files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = append_config_to_args(Args::parse())?;
    let Some(command) = args.execute_server_command else {
        bail!("You need to provide a command to start the slim server")
    };

    let mut filter = Filter::new();
    if let Some(fixture) = args.filter_fixture {
        filter = filter.fixture(&fixture)?;
    }

    if App::new(
        command,
        args.show_snoozed,
        args.pipe_output,
        args.port.unwrap_or(8085),
        args.pool_size.unwrap_or(10),
        args.recursive,
        filter,
        args.files,
    )
    .run()?
    {
        bail!("Tests executed with error");
    }

    Ok(())
}

fn append_config_to_args(mut args: Args) -> Result<Args> {
    match &args.configuration_file {
        None => Ok(args),
        Some(configuration_file) => {
            let config_file = read_to_string(configuration_file)?.parse::<Table>()?;
            args.port = args.port.or(config_file
                .get("port")
                .map(|port| port.as_integer().expect("Expect the port to be a number") as u16));
            args.pool_size = args.pool_size.or(config_file
                .get("pool_size")
                .map(|port| port.as_integer().expect("Expect the port to be a number") as u8));
            args.recursive = args.recursive
                || config_file
                    .get("recursive")
                    .map(|recursive| {
                        recursive
                            .as_bool()
                            .expect("Expect the recursive to be a boolean")
                    })
                    .unwrap_or_default();
            args.show_snoozed = args.show_snoozed
                || config_file
                    .get("show_snoozed")
                    .map(|show_snoozed| {
                        show_snoozed
                            .as_bool()
                            .expect("Expect the show_snoozed to be a boolean")
                    })
                    .unwrap_or_default();
            args.pipe_output = args.pipe_output
                || config_file
                    .get("pipe_output")
                    .map(|verbose| {
                        verbose
                            .as_bool()
                            .expect("Expect the verbose to be a boolean")
                    })
                    .unwrap_or_default();
            args.execute_server_command = args.execute_server_command.or(config_file
                .get("execute_server_command")
                .map(|command| {
                    command
                        .as_str()
                        .expect("Expect the slim server command to be a string")
                        .to_string()
                }));
            args.files = if !args.files.is_empty() {
                args.files
            } else {
                let test_dir = config_file
                    .get("test_dir")
                    .map(|test_dir| {
                        test_dir
                            .as_str()
                            .expect("Expect the test dir to be a string")
                    })
                    .ok_or(anyhow!("You need to specify a test file or a test dir"))?;
                get_list_of_files(test_dir)?
            };
            Ok(args)
        }
    }
}
