use crate::port::CyclePort;
use anyhow::{anyhow, bail, Result};
use rand::Rng;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};

pub fn build_slim_server_connector(
    command: String,
    port: u16,
    pool_size: u16,
    pipe_output: bool,
) -> Box<dyn SlimServerConnector> {
    if port == 1 {
        Box::new(StdoutSlimServerConnector {
            command,
            pipe_output,
        })
    } else {
        let mut rng = rand::thread_rng();
        Box::new(TcpSlimServerConnector {
            command,
            port: CyclePort::new(
                rng.gen_range(port..(port + (pool_size - 1))),
                port,
                pool_size,
            ),
            pipe_output,
        })
    }
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
        let stdout = build_stdio(self.pipe_output);
        let stderr = build_stdio(self.pipe_output);
        self.port.new_port();
        let child = spawn_server(
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
                bail!("Failed to connect to slim server");
            }
            sleep(sleep_time);
        };
        Ok(Box::new(TcpSlimServer { child, stream }))
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
        Ok(())
    }
}

pub struct StdoutSlimServerConnector {
    command: String,
    pipe_output: bool,
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
        let stderr_thread = std::thread::spawn(move || {
            drain_slim_stderr(child_stderr, pipe_output, io::stdout(), io::stderr())
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
            writeln!(stdout, "{line}")?;
        } else if let Some(line) = line
            .strip_prefix("SERR :")
            .or_else(|| line.strip_prefix("SERR.:"))
        {
            writeln!(stderr, "{line}")?;
        } else {
            writeln!(stdout, "{line}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::drain_slim_stderr;
    use std::io::Cursor;

    #[test]
    fn tunneled_stderr_is_split_without_duplicate_output() {
        let input = b"SOUT :one\nSOUT.:two\nSERR :bad\nSERR.:worse\nplain\n";
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        drain_slim_stderr(Cursor::new(input), true, &mut stdout, &mut stderr).unwrap();

        assert_eq!(b"one\ntwo\nplain\n", stdout.as_slice());
        assert_eq!(b"bad\nworse\n", stderr.as_slice());
    }

    #[test]
    fn tunneled_stderr_is_still_drained_when_output_is_hidden() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        drain_slim_stderr(
            Cursor::new(b"SOUT :ignored\nSERR :ignored\n"),
            false,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }
}
