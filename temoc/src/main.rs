use anyhow::{anyhow, bail, Result};
use clap::{Parser, ArgAction};
use processor::process_markdown;
use std::{
    fs::{metadata, read_dir, read_to_string},
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{exit, Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};
use toml::Table;

use slim_protocol::SlimConnection;

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
    #[arg(short, long, action(ArgAction::SetTrue))]
    recursive: Option<bool>,
    /// Show snoozed errors
    #[arg(short, long, action(ArgAction::SetTrue))]
    show_snoozed: Option<bool>,
    /// Show STDERR and STDOUT of the slim server
    #[arg(short, long, action(ArgAction::SetTrue))]
    verbose: Option<bool>,
    /// List of files to test
    files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = append_config_to_args(Args::parse())?;
    let mut slim_server = None;

    if let Some(command) = &args.execute_server_command {
        let stdout = build_stdio(&args);
        let stderr = build_stdio(&args);
        slim_server = Some(
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdout(stdout)
                .stderr(stderr)
                .spawn()?,
        );
    }

    let tcp_stream = connect_to_slim_server(args.port.unwrap_or(1), Duration::from_secs(10))?;
    let mut connection = SlimConnection::new(tcp_stream.try_clone()?, tcp_stream)?;

    let fail = process_files(
        &mut connection,
        args.recursive.unwrap_or(false),
        args.show_snoozed.unwrap_or(false),
        args.files,
    )?;
    connection.close()?;
    if fail {
        exit(1)
    }

    if let Some(mut server) = slim_server {
        if let Ok(None) = server.try_wait() {
            sleep(Duration::from_millis(500));
            server.kill()?;
        }
    }

    Ok(())
}

fn build_stdio(args: &Args) -> Stdio {
    if args.verbose.unwrap_or(false) {
        Stdio::inherit()
    } else {
        Stdio::null()
    }
}

fn process_files<R: Read, W: Write>(
    connection: &mut SlimConnection<R, W>,
    recursive: bool,
    show_snoozed: bool,
    files: Vec<PathBuf>,
) -> Result<bool> {
    let mut fail = false;
    for file in files {
        let metadata = metadata(&file)?;
        if metadata.is_dir() && recursive {
            fail |= process_files(
                connection,
                recursive,
                show_snoozed,
                get_list_of_files(file)?,
            )?;
            continue;
        }
        if metadata.is_file()
            && file
                .extension()
                .map(|ext| ext.to_ascii_lowercase() == "md")
                .unwrap_or(false)
        {
            fail |= process_markdown(connection, show_snoozed, &file)?;
        }
    }
    Ok(fail)
}

fn append_config_to_args(mut args: Args) -> Result<Args> {
    match &args.configuration_file {
        None => Ok(args),
        Some(configuration_file) => {
            let config_file = read_to_string(configuration_file)?.parse::<Table>()?;
            args.port = args.port.or(config_file
                .get("port")
                .map(|port| port.as_integer().expect("Expect the port to be a number") as u16));
            args.recursive = args
                .recursive
                .or(config_file.get("recursive").map(|recursive| {
                    recursive
                        .as_bool()
                        .expect("Expect the recursive to be a boolean")
                }));
            args.show_snoozed =
                args.show_snoozed
                    .or(config_file.get("show_snoozed").map(|show_snoozed| {
                        show_snoozed
                            .as_bool()
                            .expect("Expect the show_snoozed to be a boolean")
                    }));
            args.verbose = args.verbose.or(config_file.get("verbose").map(|verbose| {
                verbose
                    .as_bool()
                    .expect("Expect the verbose to be a boolean")
            }));
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

fn connect_to_slim_server(port: u16, time_limit: Duration) -> Result<TcpStream> {
    let start = Instant::now();
    loop {
        if let Ok(tcp_stream) = TcpStream::connect(format!("127.0.0.1:{}", port)) {
            return Ok(tcp_stream);
        }
        if start.elapsed() > time_limit {
            bail!("Failed to connect to slim server");
        }
        sleep(Duration::from_millis(300))
    }
}

fn get_list_of_files(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    Ok(read_dir(dir.as_ref())?
        .map(|file| file.map(|file| file.path().to_path_buf()))
        .collect::<Result<Vec<PathBuf>, std::io::Error>>()?)
}
