use crate::port::CyclePort;
use anyhow::{anyhow, bail, Result};
use rand::Rng;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
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
    pipe_output: bool,
}

impl SlimServerConnector for StdoutSlimServerConnector {
    fn start_and_connect(&mut self) -> Result<Box<dyn SlimServer>> {
        let child = spawn_server(
            &self.command,
            1,
            Stdio::piped(),
            Stdio::piped(),
            Stdio::piped(),
        )?;

        Ok(Box::new(StdoutSlimServer {
            child,
            pipe_output: self.pipe_output,
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
        self.child.wait()?;
        if self.pipe_output {
            let mut child_stderr = self
                .child
                .stderr
                .take()
                .ok_or(anyhow!("Failed to open stderr"))?;
            let buff_read = BufReader::new(&mut child_stderr);
            for line in buff_read.lines() {
                let line = line?;
                if let Some(line) = line.strip_prefix("SOUT :") {
                    println!("{}", line);
                } else if let Some(line) = line.strip_prefix("SOUT.:") {
                    println!("{}", line);
                } else if let Some(line) = line.strip_prefix("SERR :") {
                    eprintln!("{}", line);
                } else if let Some(line) = line.strip_prefix("SERR.:") {
                    eprintln!("{}", line);
                }
                eprintln!("{}", line);
            }
        }
        Ok(())
    }
}
