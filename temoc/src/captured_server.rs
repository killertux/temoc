//! Starts a Slim server for `--output jsonl`. Nothing it prints can reach
//! temoc's stdout, because all of it is read here and reported as events.

use crate::cancel::{ServerControl, TERMINATE_GRACE};
use crate::events::{Event, EventSink, OutputStream};
use crate::port::CyclePort;
use crate::slim_server_connector::random_cycle_port;
use anyhow::{anyhow, bail, Result};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{sleep, spawn, JoinHandle};
use std::time::{Duration, Instant};

const CONNECT_TIME_LIMIT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const OUTPUT_DRAIN_LIMIT: Duration = Duration::from_millis(500);
const EXIT_CHECK_LIMIT: Duration = Duration::from_millis(200);
const STDERR_TAIL_LINES: usize = 50;

#[derive(Default, Clone)]
struct Location {
    file: Option<String>,
    table: Option<usize>,
    row: Option<usize>,
}

/// Receives server output: keeps the end of stderr for error reports and, with
/// `-o`, forwards every line as an `output` event.
pub struct ServerLog {
    events: Arc<EventSink>,
    forward: bool,
    location: Mutex<Location>,
    stderr_tail: Mutex<VecDeque<String>>,
}

impl ServerLog {
    pub fn new(events: Arc<EventSink>, forward: bool) -> Self {
        Self {
            events,
            forward,
            location: Mutex::default(),
            stderr_tail: Mutex::default(),
        }
    }

    pub fn set_location(&self, file: Option<&str>, table: Option<usize>, row: Option<usize>) {
        *self.location.lock().expect("server log lock poisoned") = Location {
            file: file.map(str::to_string),
            table,
            row,
        };
    }

    /// Returns the end of the server's stderr and forgets it.
    pub fn take_stderr(&self) -> Option<String> {
        let mut tail = self.stderr_tail.lock().expect("server log lock poisoned");
        let text = tail.drain(..).collect::<String>();
        (!text.is_empty()).then_some(text)
    }

    fn line(&self, stream: OutputStream, text: String) {
        if stream == OutputStream::Stderr {
            let mut tail = self.stderr_tail.lock().expect("server log lock poisoned");
            if tail.len() == STDERR_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(text.clone());
        }
        if self.forward {
            let location = self
                .location
                .lock()
                .expect("server log lock poisoned")
                .clone();
            self.events.emit(&Event::Output {
                stream,
                text,
                file: location.file,
                table: location.table,
                row: location.row,
            });
        }
    }
}

fn read_lines(reader: impl Read, stream: OutputStream, tunneled: bool, log: &ServerLog) {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let text = String::from_utf8_lossy(&line).into_owned();
                let (stream, text) = if tunneled {
                    untunnel(text, stream)
                } else {
                    (stream, text)
                };
                log.line(stream, text);
            }
        }
    }
}

/// The V0.5 stdio transport tunnels fixture output through stderr.
fn untunnel(text: String, stream: OutputStream) -> (OutputStream, String) {
    for (prefix, target) in [
        ("SOUT :", OutputStream::Stdout),
        ("SOUT.:", OutputStream::Stdout),
        ("SERR :", OutputStream::Stderr),
        ("SERR.:", OutputStream::Stderr),
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return (target, rest.to_string());
        }
    }
    (stream, text)
}

pub struct CapturedServerConnector {
    command: String,
    port: Option<CyclePort>,
    log: Arc<ServerLog>,
    control: Arc<ServerControl>,
}

impl CapturedServerConnector {
    pub fn new(
        command: String,
        port: u16,
        pool_size: u16,
        log: Arc<ServerLog>,
        control: Arc<ServerControl>,
    ) -> Self {
        Self {
            command,
            port: (port != 1).then(|| random_cycle_port(port, pool_size)),
            log,
            control,
        }
    }

    pub fn start(&mut self) -> Result<CapturedServer> {
        let port = self.port.as_mut().map(|port| {
            port.new_port();
            port.to_port()
        });
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(self.command.replace("%p", &port.unwrap_or(1).to_string()))
            .stdin(if port.is_some() {
                Stdio::null()
            } else {
                Stdio::piped()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Its own group, so that stopping it also reaches whatever `sh`
            // started. temoc forwards signals itself.
            .process_group(0)
            .spawn()?;
        let process_group =
            i32::try_from(child.id()).map_err(|_| anyhow!("Unexpected server process id"))?;
        self.control.attach(process_group);

        let mut output_threads = Vec::new();
        let stderr = child
            .stderr
            .take()
            .ok_or(anyhow!("Failed to open stderr"))?;
        let log = Arc::clone(&self.log);
        let tunneled = port.is_none();
        output_threads.push(spawn(move || {
            read_lines(stderr, OutputStream::Stderr, tunneled, &log);
        }));
        if port.is_some() {
            let stdout = child
                .stdout
                .take()
                .ok_or(anyhow!("Failed to open stdout"))?;
            let log = Arc::clone(&self.log);
            output_threads.push(spawn(move || {
                read_lines(stdout, OutputStream::Stdout, false, &log);
            }));
        }

        let mut server = CapturedServer {
            child,
            stream: None,
            output_threads,
            control: Arc::clone(&self.control),
        };
        if let Some(port) = port {
            if let Err(error) = server.connect(port) {
                server.stop_now();
                return Err(error);
            }
        }
        Ok(server)
    }
}

pub struct CapturedServer {
    child: Child,
    stream: Option<TcpStream>,
    output_threads: Vec<JoinHandle<()>>,
    control: Arc<ServerControl>,
}

impl CapturedServer {
    fn connect(&mut self, port: u16) -> Result<()> {
        let start = Instant::now();
        loop {
            if self.control.is_cancelled() {
                bail!("Cancelled while connecting to the slim server");
            }
            if let Ok(stream) = TcpStream::connect(format!("127.0.0.1:{port}")) {
                // Small messages go back and forth for every row.
                stream.set_nodelay(true)?;
                self.stream = Some(stream);
                return Ok(());
            }
            // An exit status of zero may be a server that went to the
            // background, so that one keeps waiting for the port.
            if let Some(status) = self.child.try_wait()? {
                if !status.success() {
                    bail!("Slim server exited with {status} before accepting a connection");
                }
            }
            if start.elapsed() > CONNECT_TIME_LIMIT {
                bail!("Failed to connect to slim server");
            }
            sleep(POLL_INTERVAL);
        }
    }

    pub fn reader(&mut self) -> Result<Box<dyn Read>> {
        Ok(match &self.stream {
            Some(stream) => Box::new(stream.try_clone()?),
            None => Box::new(
                self.child
                    .stdout
                    .take()
                    .ok_or(anyhow!("Failed to open stdout"))?,
            ),
        })
    }

    pub fn writer(&mut self) -> Result<Box<dyn Write>> {
        Ok(match &self.stream {
            Some(stream) => Box::new(stream.try_clone()?),
            None => Box::new(
                self.child
                    .stdin
                    .take()
                    .ok_or(anyhow!("Failed to open stdin"))?,
            ),
        })
    }

    /// Whether the server is gone, allowing for the moment between its end
    /// and the connection being closed.
    pub fn has_exited(&mut self) -> bool {
        self.wait_for_exit(EXIT_CHECK_LIMIT).is_some()
    }

    /// Lets the server exit by itself after `bye`, stopping it if it doesn't.
    pub fn stop_within(mut self, grace: Duration) -> Option<ExitStatus> {
        let status = self.wait_for_exit(grace);
        if status.is_some() {
            self.finish();
            return status;
        }
        self.stop_now()
    }

    /// Stops the server and everything it started.
    pub fn stop_now(mut self) -> Option<ExitStatus> {
        self.control.signal(libc::SIGTERM);
        let status = self.wait_for_exit(TERMINATE_GRACE);
        self.control.signal(libc::SIGKILL);
        let status = status.or_else(|| self.child.wait().ok());
        self.finish();
        status
    }

    fn wait_for_exit(&mut self, limit: Duration) -> Option<ExitStatus> {
        let start = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if start.elapsed() < limit => sleep(POLL_INTERVAL),
                Ok(None) | Err(_) => return None,
            }
        }
    }

    /// Waits for the last output, but not for a stray process that still holds
    /// the pipes open.
    fn finish(&mut self) {
        self.control.detach();
        let start = Instant::now();
        while start.elapsed() < OUTPUT_DRAIN_LIMIT
            && !self.output_threads.iter().all(JoinHandle::is_finished)
        {
            sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn tunneled_lines_are_split_by_prefix() {
        for (line, stream, text) in [
            ("SOUT :a\n", OutputStream::Stdout, "a\n"),
            ("SOUT.:b\n", OutputStream::Stdout, "b\n"),
            ("SERR :c\n", OutputStream::Stderr, "c\n"),
            ("SERR.:d\n", OutputStream::Stderr, "d\n"),
            ("plain\n", OutputStream::Stderr, "plain\n"),
        ] {
            assert_eq!(
                (stream, text.to_string()),
                untunnel(line.to_string(), OutputStream::Stderr)
            );
        }
    }
}
