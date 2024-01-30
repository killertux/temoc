use crate::app::{get_list_of_files, App};
use anyhow::{anyhow, bail, Result};
use clap::Parser;
use std::{fs::read_to_string, path::PathBuf, process::exit};
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
    /// Port to connect to the slim server
    #[arg(short, long)]
    port: Option<u16>,
    /// Command to start the slim server
    #[arg(short = 'x', long)]
    execute_server_command: Option<String>,
    /// Recursively traverse files and directories to test
    #[arg(short, long)]
    recursive: bool,
    /// Show snoozed errors
    #[arg(short, long)]
    show_snoozed: bool,
    /// Show STDERR and STDOUT of the slim server
    #[arg(short, long)]
    verbose: bool,
    /// List of files to test
    files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = append_config_to_args(Args::parse())?;
    let Some(command) = args.execute_server_command else {
        bail!("You need to provide a command to start the slim server")
    };

    if App::new(
        command,
        args.show_snoozed,
        args.verbose,
        args.port.unwrap_or(1),
        args.recursive,
        args.files,
    )
    .run()
    {
        exit(1)
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
            args.verbose = args.verbose
                || config_file
                    .get("verbose")
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
