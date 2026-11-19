//! The servers of a `--output jsonl` run. Each one starts when a table first
//! needs it, so a server that no selected table uses never runs.

use crate::cancel::ServerControl;
use crate::captured_server::{CapturedServer, CapturedServerConnector, ServerLog};
use crate::events::{EventSink, RunErrorKind};
use crate::servers::{ServerConfig, Servers};
use anyhow::Result;
use slim_protocol::SlimConnection;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

/// How long a server gets to exit by itself after `bye`.
const BYE_GRACE: Duration = Duration::from_secs(5);

pub type Connection = SlimConnection<Box<dyn Read>, Box<dyn Write>>;

/// A running server and the connection to it.
struct Session {
    server: CapturedServer,
    connection: Connection,
}

impl Session {
    /// Says `bye` and lets the server exit by itself.
    fn close(self) {
        let _ = self.connection.close();
        self.server.stop_within(BYE_GRACE);
    }

    fn stop_now(self) -> Option<std::process::ExitStatus> {
        drop(self.connection);
        self.server.stop_now()
    }
}

/// Why a server can't be used for the rest of the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFailure {
    pub kind: RunErrorKind,
    /// Names the server and its command.
    pub message: String,
    pub server: String,
    /// The end of what the server wrote to stderr.
    pub stderr: Option<String>,
}

/// Why a server didn't start.
pub enum Unavailable {
    /// It failed just now.
    Failed(ServerFailure),
    /// It failed earlier in the run, with this message.
    Gone(String),
    Cancelled,
}

struct Slot {
    config: ServerConfig,
    connector: CapturedServerConnector,
    log: Arc<ServerLog>,
    session: Option<Session>,
    /// What a server that failed said. It isn't started again.
    gone: Option<String>,
}

pub struct ServerPool {
    slots: Vec<Slot>,
    control: Arc<ServerControl>,
}

impl ServerPool {
    pub fn new(servers: &Servers, events: &Arc<EventSink>, control: &Arc<ServerControl>) -> Self {
        let slots = servers
            .configs()
            .iter()
            .map(|config| {
                let log = Arc::new(ServerLog::new(
                    Arc::clone(events),
                    config.name.clone(),
                    config.pipe_output,
                ));
                Slot {
                    connector: CapturedServerConnector::new(
                        config.command.clone(),
                        config.port,
                        config.pool_size,
                        Arc::clone(&log),
                        Arc::clone(control),
                    ),
                    config: config.clone(),
                    log,
                    session: None,
                    gone: None,
                }
            })
            .collect();
        Self {
            slots,
            control: Arc::clone(control),
        }
    }

    /// With one server, losing it leaves nothing to run.
    pub fn is_single(&self) -> bool {
        self.slots.len() == 1
    }

    pub fn name(&self, server: usize) -> &str {
        &self.slots[server].config.name
    }

    pub fn file_hooks(&self, server: usize) -> Option<&str> {
        self.slots[server].config.file_hooks.as_deref()
    }

    pub fn is_running(&self, server: usize) -> bool {
        self.slots[server].session.is_some()
    }

    /// What a server that failed earlier said.
    pub fn gone(&self, server: usize) -> Option<&str> {
        self.slots[server].gone.as_deref()
    }

    pub fn connection(&mut self, server: usize) -> Option<&mut Connection> {
        let session = self.slots[server].session.as_mut()?;
        Some(&mut session.connection)
    }

    /// Starts a server, unless it is already running.
    pub fn start(&mut self, server: usize) -> Result<(), Unavailable> {
        let slot = &mut self.slots[server];
        if slot.session.is_some() {
            return Ok(());
        }
        if let Some(message) = &slot.gone {
            return Err(Unavailable::Gone(message.clone()));
        }
        let error = match connect(&mut slot.connector) {
            Ok(session) => {
                slot.session = Some(session);
                return Ok(());
            }
            Err(error) => error,
        };
        if self.control.is_cancelled() {
            return Err(Unavailable::Cancelled);
        }
        Err(Unavailable::Failed(fail(
            slot,
            RunErrorKind::ServerStartFailed,
            &format!("failed to start: {error}"),
        )))
    }

    /// Stops a server whose connection can't be used anymore, and says what
    /// happened to it.
    pub fn lost(&mut self, server: usize, kind: RunErrorKind, message: String) -> ServerFailure {
        let slot = &mut self.slots[server];
        let mut kind = kind;
        let mut message = message;
        if let Some(mut session) = slot.session.take() {
            if session.server.has_exited() {
                kind = RunErrorKind::ServerDied;
            }
            message = with_exit_status(message, session.stop_now());
        }
        fail(slot, kind, &format!("was lost: {message}"))
    }

    /// Tells where the run is, for the server output that follows.
    pub fn set_location(&self, file: Option<&str>, table: Option<usize>, row: Option<usize>) {
        for slot in &self.slots {
            slot.log.set_location(file, table, row);
        }
    }

    /// The end of what a server wrote to stderr, which is then forgotten.
    pub fn take_stderr(&self, server: usize) -> Option<String> {
        self.slots[server].log.take_stderr()
    }

    pub fn forget_stderr(&self) {
        for slot in &self.slots {
            slot.log.take_stderr();
        }
    }

    /// Ends a file: the servers that aren't persistent get their `bye`.
    pub fn finish_file(&mut self) {
        let cancelled = self.control.is_cancelled();
        for slot in &mut self.slots {
            if cancelled || !slot.config.persistent {
                stop(slot, cancelled);
            }
        }
    }

    /// Ends the run.
    pub fn finish(&mut self) {
        self.set_location(None, None, None);
        let cancelled = self.control.is_cancelled();
        for slot in &mut self.slots {
            stop(slot, cancelled);
        }
    }
}

fn stop(slot: &mut Slot, cancelled: bool) {
    if let Some(session) = slot.session.take() {
        if cancelled {
            session.stop_now();
        } else {
            session.close();
        }
    }
}

fn connect(connector: &mut CapturedServerConnector) -> Result<Session, String> {
    let mut server = connector.start().map_err(|error| format!("{error:#}"))?;
    let connection = server.reader().and_then(|reader| {
        let writer = server.writer()?;
        Ok(SlimConnection::new(reader, writer)?)
    });
    match connection {
        Ok(connection) => Ok(Session { server, connection }),
        Err(error) => Err(with_exit_status(format!("{error:#}"), server.stop_now())),
    }
}

fn fail(slot: &mut Slot, kind: RunErrorKind, message: &str) -> ServerFailure {
    let message = format!("{} {message}", slot.config.describe());
    slot.gone = Some(message.clone());
    ServerFailure {
        kind,
        message,
        server: slot.config.name.clone(),
        stderr: slot.log.take_stderr(),
    }
}

fn with_exit_status(message: String, status: Option<std::process::ExitStatus>) -> String {
    match status {
        Some(status) => format!("{message} (server {status})"),
        None => message,
    }
}
