use crate::processor::{execute_instructions_and_print_result, process_markdown_into_instructions};
use anyhow::{bail, Result};
use slim_protocol::SlimConnection;
use std::fs::{metadata, read_dir};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct App {
    command: String,
    show_snoozed: bool,
    verbose: bool,
    port: u16,
    recursive: bool,
    paths: Vec<PathBuf>,
}

impl App {
    pub fn new(
        command: String,
        show_snoozed: bool,
        verbose: bool,
        port: u16,
        recursive: bool,
        paths: Vec<PathBuf>,
    ) -> Self {
        App {
            command,
            show_snoozed,
            verbose,
            port,
            recursive,
            paths,
        }
    }

    pub fn run(self) -> Result<bool> {
        self.process_paths(self.paths)
    }

    pub fn process_paths(&self, paths: Vec<PathBuf>) -> Result<bool> {
        let mut fail = false;
        for file in self.paths {}
        Ok(fail)
    }

    pub fn process_path(&self, path: impl AsRef<Path>) -> Result<bool> {
        let metadata = metadata(&path)?;
        if metadata.is_dir() && self.recursive {
            return self.process_paths(get_list_of_files(path)?);
        }
        if metadata.is_file()
            && Self::is_markdown_format(&path)
        {
            return self.process_file(path);
        }
        Ok(true)
    }

    fn is_markdown_format(path: &impl AsRef<Path> + Sized) -> _ {
        path
            .extension()
            .map(|ext| ext.to_ascii_lowercase() == "md")
            .unwrap_or(false)
    }

    fn process_file(&self, file: &PathBuf) -> Result<bool> {
        let (instructions, expected_result) = process_markdown_into_instructions(&file)?;
        if instructions.is_empty() {
            println!("NONE");
            return Ok(false);
        }
        let stdout = build_stdio(self.verbose);
        let stderr = build_stdio(self.verbose);
        let mut slim_server = Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .stdout(stdout)
            .stderr(stderr)
            .spawn()?;
        let tcp_stream = connect_to_slim_server(self.port, Duration::from_secs(10))?;
        let mut connection = SlimConnection::new(tcp_stream.try_clone()?, tcp_stream)?;
        let fail = execute_instructions_and_print_result(
            &mut connection,
            &file.to_string_lossy(),
            instructions,
            expected_result,
            self.show_snoozed,
        )?;
        connection.close()?;
        slim_server.wait()?;
        return Ok(fail);
    }
}

pub fn get_list_of_files(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    Ok(read_dir(dir.as_ref())?
        .map(|file| file.map(|file| file.path().to_path_buf()))
        .collect::<Result<Vec<PathBuf>, std::io::Error>>()?)
}

fn build_stdio(verbose: bool) -> Stdio {
    if verbose {
        Stdio::inherit()
    } else {
        Stdio::null()
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
    }
}
