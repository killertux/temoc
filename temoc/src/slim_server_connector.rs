use crate::port::CyclePort;
use anyhow::{anyhow, bail, Result};
use rand::Rng;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};

/// `output_prefix` starts every line of server output that is piped. It
/// tells the servers apart when there are several, and is empty otherwise.
pub fn build_slim_server_connector(
    command: String,
    port: u16,
    pool_size: u16,
    pipe_output: bool,
    output_prefix: String,
) -> Box<dyn SlimServerConnector> {
    if port == 1 {
        Box::new(StdoutSlimServerConnector {
            command,
            pipe_output,
            output_prefix,
        })
    } else {
        Box::new(TcpSlimServerConnector {
            command,
            port: random_cycle_port(port, pool_size),
            pipe_output,
            output_prefix,
        })
    }
}

pub fn random_cycle_port(port: u16, pool_size: u16) -> CyclePort {
    let mut rng = rand::thread_rng();
    let available = (u16::MAX - port).saturating_add(1);
    let pool_size = pool_size.max(1).min(available);
    let offset = rng.gen_range(0..pool_size);
    CyclePort::new(port + offset, port, pool_size)
}

pub trait SlimServerConnector {
    fn start_and_connect(&mut self) -> Result<Box<dyn SlimServer>>;
}

pub trait SlimServer {
    fn reader(&mut self) -> Result<Box<dyn Read>>;
    fn writer(&mut self) -> Result<Box<dyn Write>>;
    fn close(&mut self) -> Result<()>;
}

pub struct TcpSlimServerConnector {
    command: String,
    port: CyclePort,
    pipe_output: bool,
    output_prefix: String,
}

fn spawn_server(
    command: &str,
    port: u16,
    stdout: Stdio,
    stderr: Stdio,
    stdin: Stdio,
) -> Result<Child> {
    let child = Command::new("sh")
        .arg("-c")
        .arg(command.replace("%p", &port.to_string()))
        .stdout(stdout)
        .stderr(stderr)
        .stdin(stdin)
        .spawn()?;
    Ok(child)
}

impl SlimServerConnector for TcpSlimServerConnector {
    fn start_and_connect(&mut self) -> Result<Box<dyn SlimServer>> {
        // The output can only get a prefix if it goes through temoc.
        let prefixed = self.pipe_output && !self.output_prefix.is_empty();
        let stdio = || {
            if prefixed {
                Stdio::piped()
            } else {
                build_stdio(self.pipe_output)
            }
        };
        let (stdout, stderr) = (stdio(), stdio());
        self.port.new_port();
        let mut child = spawn_server(
            &self.command,
            self.port.to_port(),
            stdout,
            stderr,
            Stdio::null(),
        )?;
        let start = Instant::now();
        let time_limit = Duration::from_secs(10);
        let sleep_time = Duration::from_millis(100);
        let stream = loop {
            if let Ok(tcp_stream) = TcpStream::connect(format!("127.0.0.1:{}", self.port.to_port()))
            {
                break tcp_stream;
            }
            if start.elapsed() > time_limit {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Failed to connect to slim server");
            }
            sleep(sleep_time);
        };
        let mut output_threads = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            let prefix = self.output_prefix.clone();
            output_threads.push(std::thread::spawn(move || {
                forward_lines(stdout, &prefix, io::stdout())
            }));
        }
        if let Some(stderr) = child.stderr.take() {
            let prefix = self.output_prefix.clone();
            output_threads.push(std::thread::spawn(move || {
                forward_lines(stderr, &prefix, io::stderr())
            }));
        }
        Ok(Box::new(TcpSlimServer {
            child,
            stream,
            output_threads,
        }))
    }
}

fn build_stdio(pipe_output: bool) -> Stdio {
    if pipe_output {
        Stdio::inherit()
    } else {
        Stdio::null()
    }
}

struct TcpSlimServer {
    child: Child,
    stream: TcpStream,
    output_threads: Vec<JoinHandle<io::Result<()>>>,
}

impl SlimServer for TcpSlimServer {
    fn reader(&mut self) -> Result<Box<dyn Read>> {
        Ok(Box::new(self.stream.try_clone()?))
    }

    fn writer(&mut self) -> Result<Box<dyn Write>> {
        Ok(Box::new(self.stream.try_clone()?))
    }

    fn close(&mut self) -> Result<()> {
        self.child.wait()?;
        for thread in self.output_threads.drain(..) {
            thread
                .join()
                .map_err(|_| anyhow!("server output thread panicked"))??;
        }
        Ok(())
    }
}

/// Copies the output of a server, line by line, with the prefix of that
/// server in front of each one.
fn forward_lines(reader: impl Read, prefix: &str, mut writer: impl Write) -> io::Result<()> {
    for line in BufReader::new(reader).lines() {
        writeln!(writer, "{prefix}{}", line?)?;
    }
    Ok(())
}

pub struct StdoutSlimServerConnector {
    command: String,
    pipe_output: bool,
    output_prefix: String,
}

struct StdoutSlimServer {
    child: Child,
    stderr_thread: Option<JoinHandle<io::Result<()>>>,
}

impl SlimServerConnector for StdoutSlimServerConnector {
    fn start_and_connect(&mut self) -> Result<Box<dyn SlimServer>> {
        let mut child = spawn_server(
            &self.command,
            1,
            Stdio::piped(),
            Stdio::piped(),
            Stdio::piped(),
        )?;
        let child_stderr = child
            .stderr
            .take()
            .ok_or(anyhow!("Failed to open stderr"))?;
        let pipe_output = self.pipe_output;
        let prefix = self.output_prefix.clone();
        let stderr_thread = std::thread::spawn(move || {
            drain_slim_stderr(
                child_stderr,
                pipe_output,
                &prefix,
                io::stdout(),
                io::stderr(),
            )
        });

        Ok(Box::new(StdoutSlimServer {
            child,
            stderr_thread: Some(stderr_thread),
        }))
    }
}

impl SlimServer for StdoutSlimServer {
    fn reader(&mut self) -> Result<Box<dyn Read>> {
        Ok(Box::new(
            self.child
                .stdout
                .take()
                .ok_or(anyhow!("Failed to open stdout"))?,
        ))
    }

    fn writer(&mut self) -> Result<Box<dyn Write>> {
        Ok(Box::new(
            self.child
                .stdin
                .take()
                .ok_or(anyhow!("Failed to open stdout"))?,
        ))
    }

    fn close(&mut self) -> Result<()> {
        let wait_result = self.child.wait().map(|_| ());
        let drain_result = self
            .stderr_thread
            .take()
            .ok_or(anyhow!("stderr drain thread is missing"))?
            .join()
            .map_err(|_| anyhow!("stderr drain thread panicked"))?;
        wait_result?;
        drain_result?;
        Ok(())
    }
}

fn drain_slim_stderr<R, O, E>(
    reader: R,
    pipe_output: bool,
    prefix: &str,
    mut stdout: O,
    mut stderr: E,
) -> io::Result<()>
where
    R: Read,
    O: Write,
    E: Write,
{
    for line in BufReader::new(reader).lines() {
        let line = line?;
        if !pipe_output {
            continue;
        }
        if let Some(line) = line
            .strip_prefix("SOUT :")
            .or_else(|| line.strip_prefix("SOUT.:"))
        {
            writeln!(stdout, "{prefix}{line}")?;
        } else if let Some(line) = line
            .strip_prefix("SERR :")
            .or_else(|| line.strip_prefix("SERR.:"))
        {
            writeln!(stderr, "{prefix}{line}")?;
        } else {
            writeln!(stdout, "{prefix}{line}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_slim_server_connector, drain_slim_stderr, forward_lines};
    use std::io::Cursor;

    #[test]
    fn tunneled_stderr_is_split_without_duplicate_output() {
        let input = b"SOUT :one\nSOUT.:two\nSERR :bad\nSERR.:worse\nplain\n";
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        drain_slim_stderr(Cursor::new(input), true, "", &mut stdout, &mut stderr).unwrap();

        assert_eq!(b"one\ntwo\nplain\n", stdout.as_slice());
        assert_eq!(b"bad\nworse\n", stderr.as_slice());
    }

    #[test]
    fn piped_lines_start_with_the_prefix_of_their_server() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        drain_slim_stderr(
            Cursor::new(b"SOUT :one\nSERR :bad\nplain\n"),
            true,
            "[machines] ",
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(b"[machines] one\n[machines] plain\n", stdout.as_slice());
        assert_eq!(b"[machines] bad\n", stderr.as_slice());

        let mut copied = Vec::new();
        forward_lines(Cursor::new(b"a\nb\n"), "[machines] ", &mut copied).unwrap();
        assert_eq!(b"[machines] a\n[machines] b\n", copied.as_slice());
    }

    #[test]
    fn tunneled_stderr_is_still_drained_when_output_is_hidden() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        drain_slim_stderr(
            Cursor::new(b"SOUT :ignored\nSERR :ignored\n"),
            false,
            "",
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(stdout, Vec::<u8>::new());
        assert_eq!(stderr, Vec::<u8>::new());
    }

    #[test]
    fn tcp_connector_accepts_a_single_port_pool() {
        let _connector = build_slim_server_connector("false".into(), 8085, 1, false, String::new());
    }
}
