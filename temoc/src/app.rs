use crate::processor::{
    execute_instructions, file_hooks, print_fail_or_ok, process_markdown_into_instructions, Batch,
    Failures, Filter, State,
};
use crate::slim_server_connector::{SlimServer, SlimServerConnector};
use anyhow::Result;
use slim_protocol::SlimConnection;
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

pub struct Options {
    pub show_snoozed: bool,
    pub recursive: bool,
    pub extension: String,
    pub filter: Filter,
    /// Keeps one server for the whole run, instead of one for each file.
    pub persistent_server: bool,
    /// The fixture whose `beginFile` and `endFile` surround every file.
    pub file_hooks: Option<String>,
    pub paths: Vec<PathBuf>,
}

pub struct App {
    options: Options,
    slim_server_connector: Box<dyn SlimServerConnector>,
    session: Option<Session>,
}

impl App {
    pub fn new(options: Options, slim_server_connector: Box<dyn SlimServerConnector>) -> Self {
        App {
            options,
            slim_server_connector,
            session: None,
        }
    }

    pub fn run(mut self) -> Result<bool> {
        let result = self.process_paths(self.options.paths.clone());
        let closed = self.session.take().map_or(Ok(()), Session::close);
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
        let (instructions, expected_result) = process_markdown_into_instructions(&file, filter)?;
        if instructions.is_empty() {
            println!("NONE");
            return Ok(false);
        }
        let mut session = match self.session.take() {
            Some(session) => session,
            None => self.connect()?,
        };
        let file = file.as_ref().to_string_lossy();
        let file_batch = Batch {
            instructions,
            expected_result,
        };
        let failures = self.run_file(&mut session.connection, &file, file_batch);
        if self.options.persistent_server && failures.is_ok() {
            self.session = Some(session);
        } else {
            let closed = session.close();
            if failures.is_ok() {
                closed?;
            }
        }
        Ok(print_fail_or_ok(self.options.show_snoozed, failures?))
    }

    fn connect(&mut self) -> Result<Session> {
        let mut server = self.slim_server_connector.start_and_connect()?;
        let connection = SlimConnection::new(server.reader()?, server.writer()?)?;
        Ok(Session { server, connection })
    }

    /// Runs a file between its hooks. The tables don't run when the hooks
    /// fixture can't be made or `beginFile` fails, and `endFile` runs whenever
    /// the fixture was made.
    fn run_file(&self, connection: &mut Connection, file: &str, tables: Batch) -> Result<Failures> {
        let mut state = State::default();
        let mut send = |batch: Batch| execute_instructions(connection, file, batch, &mut state);
        let Some(class) = &self.options.file_hooks else {
            return send(tables);
        };
        let hooks = file_hooks(class, file);
        let mut failures = send(hooks.make)?;
        if !failures.is_empty() {
            return Ok(failures);
        }
        failures = send(hooks.begin)?;
        if failures.is_empty() {
            failures = send(tables)?;
        }
        failures.extend(send(hooks.end)?);
        Ok(failures)
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
