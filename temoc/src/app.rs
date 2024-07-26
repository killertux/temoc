use crate::processor::{
    execute_instructions_and_print_result, process_markdown_into_instructions, Filter,
};
use anyhow::{bail, Result};
use slim_protocol::SlimConnection;
use std::fs::{metadata, read_dir};
use std::net::TcpStream;
use std::num::ParseIntError;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

pub struct App {
    command: String,
    show_snoozed: bool,
    pipe_output: bool,
    base_port: u16,
    current_port: u16,
    pool_size: u8,
    recursive: bool,
    filter: Filter,
    paths: Vec<PathBuf>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: String,
        show_snoozed: bool,
        pipe_output: bool,
        base_port: u16,
        pool_size: u8,
        recursive: bool,
        filter: Filter,
        paths: Vec<PathBuf>,
    ) -> Self {
        App {
            command,
            show_snoozed,
            pipe_output,
            base_port,
            current_port: base_port,
            pool_size,
            recursive,
            filter,
            paths,
        }
    }

    pub fn run(mut self) -> Result<bool> {
        self.process_paths(self.paths.clone())
    }

    pub fn process_paths(&mut self, paths: Vec<PathBuf>) -> Result<bool> {
        let mut fail = false;
        for path in paths {
            fail |= self.process_path(path)?;
        }
        Ok(fail)
    }

    pub fn process_path(&mut self, path: impl AsRef<Path>) -> Result<bool> {
        let mut path = path.as_ref().to_path_buf();
        let mut filter = self.filter.clone();
        if let Ok(line) = Self::get_line_from_path(&path) {
            if let Some(remaining) = path
                .to_string_lossy()
                .strip_suffix(&format!(":{line}"))
            {
                path = PathBuf::from(remaining);
                filter = filter.line(line);
            }
        }
        let metadata = metadata(&path)?;
        if metadata.is_dir() && self.recursive {
            return self.process_paths(get_list_of_files(&path)?);
        }
        if metadata.is_file() && Self::is_markdown_format(&path) {
            return self.process_file(&path, filter);
        }
        Ok(false)
    }

    fn get_line_from_path(path: &impl AsRef<Path>) -> Result<usize, ParseIntError> {
        let line = path
            .as_ref()
            .display()
            .to_string()
            .chars()
            .rev()
            .take_while(|c| c.is_numeric())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        line.parse::<usize>()
    }

    fn is_markdown_format(path: impl AsRef<Path>) -> bool {
        path.as_ref()
            .extension()
            .map(|ext| ext.to_ascii_lowercase() == "md")
            .unwrap_or(false)
    }

    fn process_file(&mut self, file: impl AsRef<Path>, filter: Filter) -> Result<bool> {
        let (instructions, expected_result) = process_markdown_into_instructions(&file, &filter)?;
        if instructions.is_empty() {
            println!("NONE");
            return Ok(false);
        }
        let mut slim_server = self.start_slim_server()?;
        let tcp_stream = self.connect_to_slim_server(Duration::from_secs(10))?;
        let mut connection = SlimConnection::new(tcp_stream.try_clone()?, tcp_stream)?;
        let fail = execute_instructions_and_print_result(
            &mut connection,
            &file.as_ref().to_string_lossy(),
            instructions,
            expected_result,
            self.show_snoozed,
        )?;
        connection.close()?;
        slim_server.wait()?;
        Ok(fail)
    }

    fn start_slim_server(&self) -> Result<Child> {
        let stdout = self.build_stdio();
        let stderr = self.build_stdio();
        Ok(Command::new("sh")
            .arg("-c")
            .arg(self.command.replace("%p", &self.current_port.to_string()))
            .stdout(stdout)
            .stderr(stderr)
            .spawn()?)
    }

    fn connect_to_slim_server(&mut self, time_limit: Duration) -> Result<TcpStream> {
        let start = Instant::now();
        loop {
            if let Ok(tcp_stream) = TcpStream::connect(format!("127.0.0.1:{}", self.current_port)) {
                self.cycle_port();
                return Ok(tcp_stream);
            }
            if start.elapsed() > time_limit {
                bail!("Failed to connect to slim server");
            }
            sleep(Duration::from_millis(100));
        }
    }

    fn cycle_port(&mut self) {
        self.current_port += 1;
        if self.current_port > self.base_port + self.pool_size as u16 {
            self.current_port = self.base_port;
        }
    }

    fn build_stdio(&self) -> Stdio {
        if self.pipe_output {
            Stdio::inherit()
        } else {
            Stdio::null()
        }
    }
}

pub fn get_list_of_files(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    Ok(read_dir(dir.as_ref())?
        .map(|file| file.map(|file| file.path().to_path_buf()))
        .collect::<Result<Vec<PathBuf>, std::io::Error>>()?)
}
