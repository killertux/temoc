use crate::processor::{
    execute_instructions, file_hooks, load_file_segments, print_fail_or_ok, Batch, Failures,
    Filter, Segment, Snooze, State,
};
use crate::servers::{ServerConfig, Servers};
use crate::slim_server_connector::{build_slim_server_connector, SlimServer, SlimServerConnector};
use anyhow::{anyhow, Result};
use slim_protocol::{SendInstructionsError, SlimConnection};
use std::collections::HashSet;
use std::fs::{metadata, read_dir};
use std::io::{Read, Write};
use std::num::ParseIntError;
use std::path::{Path, PathBuf};

type Connection = SlimConnection<Box<dyn Read>, Box<dyn Write>>;

/// A running server and the connection to it.
struct Session {
    server: Box<dyn SlimServer>,
    connection: Connection,
}

impl Session {
    fn close(mut self) -> Result<()> {
        self.connection.close()?;
        self.server.close()
    }
}

/// A configured server, which runs from the first table that needs it.
struct Slot {
    config: ServerConfig,
    connector: Box<dyn SlimServerConnector>,
    session: Option<Session>,
    /// What a server that failed said. It isn't started again.
    gone: Option<String>,
}

impl Slot {
    fn connect(&mut self) -> Result<Session> {
        let mut server = self.connector.start_and_connect()?;
        let connection = SlimConnection::new(server.reader()?, server.writer()?)?;
        Ok(Session { server, connection })
    }
}

/// What running a file has come to so far.
struct FileRun<'a> {
    file: &'a str,
    state: State,
    failures: Failures,
    /// The servers whose tables don't run in this file anymore.
    blocked: HashSet<usize>,
    /// The `endFile` of every server whose hooks fixture was made.
    ends: Vec<(usize, Batch)>,
}

pub struct Options {
    pub show_snoozed: bool,
    pub recursive: bool,
    pub extension: String,
    pub filter: Filter,
    pub paths: Vec<PathBuf>,
}

pub struct App {
    options: Options,
    servers: Servers,
    slots: Vec<Slot>,
}

impl App {
    pub fn new(options: Options, servers: Servers) -> Self {
        let several = servers.len() > 1;
        let slots = servers
            .configs()
            .iter()
            .map(|config| Slot {
                connector: build_slim_server_connector(
                    config.command.clone(),
                    config.port,
                    config.pool_size,
                    config.pipe_output,
                    if several {
                        format!("[{}] ", config.name)
                    } else {
                        String::new()
                    },
                ),
                config: config.clone(),
                session: None,
                gone: None,
            })
            .collect();
        App {
            options,
            servers,
            slots,
        }
    }

    pub fn run(mut self) -> Result<bool> {
        let result = self.process_paths(self.options.paths.clone());
        let closed = self.close_sessions(true);
        let fail = result?;
        closed?;
        Ok(fail)
    }

    pub fn process_paths(&mut self, paths: Vec<PathBuf>) -> Result<bool> {
        let mut fail = false;
        for path in paths {
            fail |= self.process_path(path)?;
        }
        Ok(fail)
    }

    pub fn process_path(&mut self, path: impl AsRef<Path>) -> Result<bool> {
        let (path, line) = split_selector(path.as_ref());
        let mut filter = self.options.filter.clone();
        if let Some(line) = line {
            filter = filter.line(line);
        }
        let metadata = metadata(&path)?;
        if metadata.is_dir() && self.options.recursive {
            return self.process_paths(get_list_of_files(&path)?);
        }
        if metadata.is_file() && has_extension(&path, &self.options.extension) {
            return self.process_file(&path, &filter);
        }
        Ok(false)
    }

    fn process_file(&mut self, file: impl AsRef<Path>, filter: &Filter) -> Result<bool> {
        let segments = load_file_segments(&file, filter, &self.servers)?;
        if segments.is_empty() {
            println!("NONE");
            return Ok(false);
        }
        let file = file.as_ref().to_string_lossy();
        let failures = self.run_file(&file, segments);
        // After an error nothing else runs, so no server is kept.
        let closed = self.close_sessions(failures.is_err());
        let failures = failures?;
        closed?;
        Ok(print_fail_or_ok(self.options.show_snoozed, failures))
    }

    /// Closes the servers that only last for a file, or every server.
    fn close_sessions(&mut self, all: bool) -> Result<()> {
        let mut result = Ok(());
        for slot in &mut self.slots {
            if all || !slot.config.persistent {
                if let Some(session) = slot.session.take() {
                    result = result.and(session.close());
                }
            }
        }
        result
    }

    /// Runs a file, each part on its server. A server starts at the first
    /// table that needs it, which is also when its file hooks begin: the
    /// tables of a server don't run when its hooks fixture can't be made or
    /// `beginFile` fails, and `endFile` runs whenever the fixture was made.
    fn run_file(&mut self, file: &str, segments: Vec<Segment>) -> Result<Failures> {
        let used: HashSet<usize> = segments.iter().map(|segment| segment.server).collect();
        let mut run = FileRun {
            file,
            state: State::new(used.len() > 1),
            failures: Vec::new(),
            blocked: HashSet::new(),
            ends: Vec::new(),
        };
        let mut entered = HashSet::new();
        for segment in segments {
            if entered.insert(segment.server) {
                self.begin_file(&mut run, segment.server)?;
            }
            if !run.blocked.contains(&segment.server) {
                self.send(&mut run, segment.server, segment.batch)?;
            }
        }
        for (server, end) in std::mem::take(&mut run.ends) {
            if self.slots[server].gone.is_none() {
                self.send(&mut run, server, end)?;
            }
        }
        Ok(run.failures)
    }

    /// Calls the file hooks of a server, if it has any, before its first
    /// table of the file.
    fn begin_file(&mut self, run: &mut FileRun, server: usize) -> Result<()> {
        let Some(class) = self.slots[server].config.file_hooks.clone() else {
            return Ok(());
        };
        let hooks = file_hooks(&class, run.file);
        if !self.send(run, server, hooks.make)? {
            run.blocked.insert(server);
            return Ok(());
        }
        run.ends.push((server, hooks.end));
        if !self.send(run, server, hooks.begin)? {
            run.blocked.insert(server);
        }
        Ok(())
    }

    /// Sends a batch to a server, starting it if needed, and keeps what went
    /// wrong. Returns whether nothing did.
    fn send(&mut self, run: &mut FileRun, server: usize, batch: Batch) -> Result<bool> {
        let slot = &mut self.slots[server];
        if let Some(message) = slot.gone.clone() {
            return self.unusable(run, server, message);
        }
        if slot.session.is_none() {
            match slot.connect() {
                Ok(session) => slot.session = Some(session),
                Err(error) => {
                    let message = format!("{} failed to start: {error:#}", slot.config.describe());
                    return self.unusable(run, server, message);
                }
            }
        }
        let Some(session) = slot.session.as_mut() else {
            return Ok(false);
        };
        run.state.on_server(&slot.config.name);
        match execute_instructions(&mut session.connection, run.file, batch, &mut run.state) {
            Ok(failures) => {
                let passed = failures.is_empty();
                run.failures.extend(failures);
                Ok(passed)
            }
            Err(error) if error.is::<SendInstructionsError>() => {
                let message = format!("{} was lost: {error:#}", slot.config.describe());
                // Its connection is no use anymore, so there is no `bye`.
                slot.session = None;
                self.unusable(run, server, message)
            }
            Err(error) => Err(error),
        }
    }

    /// Fails the tables of a server that can't be used, once for each file.
    /// With no other server, the run stops.
    fn unusable(&mut self, run: &mut FileRun, server: usize, message: String) -> Result<bool> {
        if self.slots.len() == 1 {
            return Err(anyhow!(message));
        }
        self.slots[server].gone = Some(message.clone());
        if run.blocked.insert(server) {
            run.failures.push((message, Snooze::not_snooze()));
        }
        Ok(false)
    }
}

/// Splits `file.md:12` into the path and the selected line.
pub fn split_selector(path: &Path) -> (PathBuf, Option<usize>) {
    if let Ok(line) = get_line_from_path(path) {
        if let Some(remaining) = path.to_string_lossy().strip_suffix(&format!(":{line}")) {
            return (PathBuf::from(remaining), Some(line));
        }
    }
    (path.to_path_buf(), None)
}

fn get_line_from_path(path: &Path) -> Result<usize, ParseIntError> {
    let line = path
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

pub fn has_extension(path: &Path, extension: &str) -> bool {
    path.to_string_lossy().to_lowercase().ends_with(extension)
}

pub fn get_list_of_files(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    Ok(read_dir(dir.as_ref())?
        .map(|file| file.map(|file| file.path().clone()))
        .collect::<Result<Vec<PathBuf>, std::io::Error>>()?)
}
