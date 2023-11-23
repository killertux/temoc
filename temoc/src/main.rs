use anyhow::{anyhow, bail, Result};
use clap::Parser;
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
    #[arg(short, long)]
    recursive: bool,
    /// Show snoozed errors
    #[arg(short, long, default_value_t = false)]
    show_snoozed: bool,
    /// List of files to test
    files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = append_config_to_args(Args::parse())?;
    let mut slim_server = None;

    if let Some(command) = args.execute_server_command {
        slim_server = Some(
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
    }

    let tcp_stream = connect_to_slim_server(args.port.unwrap_or(1), Duration::from_secs(10))?;
    let mut connection = SlimConnection::new(tcp_stream.try_clone()?, tcp_stream)?;

    let fail = process_files(
        &mut connection,
        args.recursive,
        args.show_snoozed,
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
            args.recursive = args.recursive
                || config_file
                    .get("recursive")
                    .map(|recursive| {
                        recursive
                            .as_bool()
                            .expect("Expect the recursive to be a boolean")
                    })
                    .unwrap_or(false);
            args.execute_server_command = args.execute_server_command.or(config_file
                .get("slim_server_command")
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
