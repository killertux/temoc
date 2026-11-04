//! The `--output jsonl` run. `readme.md` documents what it prints.

use crate::app::{get_list_of_files, has_extension, split_selector};
use crate::cancel::{cancel_on_signals, ServerControl};
use crate::captured_server::{CapturedServerConnector, ServerLog};
use crate::events::{Counts, Event, EventSink, RunErrorKind, Status, SCHEMA_VERSION};
use crate::processor::stream::{file_started, FileRunner, Halt};
use crate::processor::{load_file_plan, slim_plan::FilePlan, Filter};
use anyhow::Result;
use slim_protocol::SlimConnection;
use std::fs::metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a server gets to exit by itself after `bye`.
const BYE_GRACE: Duration = Duration::from_secs(5);

pub struct JsonlOptions {
    pub command: String,
    pub port: u16,
    pub pool_size: u16,
    pub pipe_output: bool,
    pub recursive: bool,
    pub extension: String,
    pub filter: Filter,
    pub paths: Vec<PathBuf>,
}

/// Runs everything and returns the exit code.
pub fn run(options: Result<JsonlOptions>) -> i32 {
    let started = Instant::now();
    let control = Arc::new(ServerControl::default());
    let events = Arc::new(EventSink::new(
        Box::new(std::io::stdout()),
        Arc::clone(&control),
    ));
    events.emit(&Event::RunStarted {
        schema: SCHEMA_VERSION,
        temoc_version: env!("CARGO_PKG_VERSION"),
    });

    let mut status = Status::Passed;
    let mut counts = Counts::default();
    match options.and_then(|options| {
        cancel_on_signals(Arc::clone(&control))?;
        Ok(options)
    }) {
        Err(error) => {
            events.emit(&Event::RunError {
                kind: RunErrorKind::BadConfig,
                message: format!("{error:#}"),
                file: None,
                stderr: None,
            });
            status = Status::Error;
        }
        Ok(options) => {
            let mut run = JsonlRun::new(options, &events, &control);
            run.process_all();
            status = status.worst(run.status);
            counts = run.counts;
        }
    }
    if control.is_cancelled() {
        status = Status::Cancelled;
    }

    let exit_code = i32::from(status.fails_the_run());
    events.finish(&Event::RunFinished {
        status,
        exit_code,
        duration_ms: started.elapsed().as_millis(),
        counts,
    });
    exit_code
}

struct JsonlRun<'a> {
    options: JsonlOptions,
    connector: CapturedServerConnector,
    log: Arc<ServerLog>,
    events: &'a EventSink,
    control: &'a ServerControl,
    status: Status,
    counts: Counts,
    stopped: bool,
}

impl<'a> JsonlRun<'a> {
    fn new(
        options: JsonlOptions,
        events: &'a Arc<EventSink>,
        control: &'a Arc<ServerControl>,
    ) -> Self {
        let log = Arc::new(ServerLog::new(Arc::clone(events), options.pipe_output));
        let connector = CapturedServerConnector::new(
            options.command.clone(),
            options.port,
            options.pool_size,
            Arc::clone(&log),
            Arc::clone(control),
        );
        Self {
            options,
            connector,
            log,
            events,
            control,
            status: Status::Passed,
            counts: Counts::default(),
            stopped: false,
        }
    }

    fn process_all(&mut self) {
        for path in self.options.paths.clone() {
            self.process_path(&path);
        }
    }

    fn process_path(&mut self, path: &Path) {
        if self.stopped || self.control.is_cancelled() {
            return;
        }
        let (path, line) = split_selector(path);
        let file = path.display().to_string();
        let metadata = match metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                return self.run_error(
                    RunErrorKind::FileError,
                    format!("{file}: {error}"),
                    Some(file),
                );
            }
        };
        if metadata.is_dir() && self.options.recursive {
            match get_list_of_files(&path) {
                Ok(paths) => paths.iter().for_each(|path| self.process_path(path)),
                Err(error) => self.run_error(
                    RunErrorKind::FileError,
                    format!("{file}: {error:#}"),
                    Some(file),
                ),
            }
        } else if metadata.is_file() && has_extension(&path, &self.options.extension) {
            self.process_file(&path, &file, line);
        } else if line.is_some() {
            self.no_match(file, line);
        }
    }

    fn process_file(&mut self, path: &Path, file: &str, line: Option<usize>) {
        let mut filter = self.options.filter.clone();
        if let Some(line) = line {
            filter = filter.line(line);
        }
        match load_file_plan(path, &filter) {
            Err(error) => self.run_error(
                RunErrorKind::ParseError,
                format!("{error:#}"),
                Some(file.to_string()),
            ),
            Ok(None) if line.is_some() => self.no_match(file.to_string(), line),
            Ok(None) => {
                self.events.emit(&Event::FileStarted {
                    file: file.to_string(),
                    tables: Vec::new(),
                });
                self.events.emit(&Event::FileFinished {
                    file: file.to_string(),
                    status: Status::Passed,
                    duration_ms: 0,
                    counts: Counts::default(),
                });
            }
            Ok(Some(plan)) => self.run_plan(file, plan),
        }
    }

    fn run_plan(&mut self, file: &str, plan: FilePlan) {
        let started = Instant::now();
        self.events.emit(&file_started(file, &plan));
        self.log.set_location(Some(file), None, None);
        self.log.take_stderr();
        let events = self.events;
        let control = self.control;
        let log = Arc::clone(&self.log);
        let mut runner = FileRunner::new(file, events, &log, control);

        let halt = self.execute(&mut runner, plan);
        if matches!(halt, Some(Halt::Connection { .. })) {
            runner.mark_error();
        }
        self.status = self.status.worst(runner.status());
        self.counts.merge(runner.counts());
        events.emit(&Event::FileFinished {
            file: file.to_string(),
            status: runner.status(),
            duration_ms: started.elapsed().as_millis(),
            counts: runner.counts(),
        });

        match halt {
            Some(Halt::Connection { kind, message }) => {
                self.run_error(kind, message, Some(file.to_string()));
                self.stopped = true;
            }
            Some(Halt::Suite(message)) => {
                self.run_error(RunErrorKind::SuiteAborted, message, Some(file.to_string()));
                self.stopped = true;
            }
            Some(Halt::Cancelled) => self.stopped = true,
            Some(Halt::File | Halt::Table) | None => {}
        }
    }

    fn execute(&mut self, runner: &mut FileRunner<'_>, plan: FilePlan) -> Option<Halt> {
        let mut server = match self.connector.start() {
            Ok(server) => server,
            Err(error) => {
                runner.skip(plan);
                return Some(self.start_failed(format!("{error:#}")));
            }
        };
        let connection = server.reader().and_then(|reader| {
            let writer = server.writer()?;
            Ok(SlimConnection::new(reader, writer)?)
        });
        let mut connection = match connection {
            Ok(connection) => connection,
            Err(error) => {
                let status = server.stop_now();
                runner.skip(plan);
                return Some(self.start_failed(with_exit_status(format!("{error:#}"), status)));
            }
        };

        let halt = runner.run(&mut connection, plan);
        match halt {
            Some(Halt::Connection { kind, message }) => {
                // A server that dies mid message looks like a framing error.
                // This comes first because `bye` would end a healthy server.
                let kind = if server.has_exited() {
                    RunErrorKind::ServerDied
                } else {
                    kind
                };
                drop(connection);
                let status = server.stop_now();
                Some(Halt::Connection {
                    kind,
                    message: with_exit_status(message, status),
                })
            }
            Some(Halt::Cancelled) => {
                drop(connection);
                server.stop_now();
                Some(Halt::Cancelled)
            }
            halt => {
                let _ = connection.close();
                server.stop_within(BYE_GRACE);
                halt
            }
        }
    }

    fn start_failed(&self, message: String) -> Halt {
        if self.control.is_cancelled() {
            return Halt::Cancelled;
        }
        Halt::Connection {
            kind: RunErrorKind::ServerStartFailed,
            message,
        }
    }

    fn no_match(&mut self, file: String, line: Option<usize>) {
        let line = line.map(|line| format!(":{line}")).unwrap_or_default();
        self.run_error(
            RunErrorKind::NoMatch,
            format!("No decision table or row at {file}{line}"),
            Some(file),
        );
    }

    fn run_error(&mut self, kind: RunErrorKind, message: String, file: Option<String>) {
        self.status = self.status.worst(Status::Error);
        self.events.emit(&Event::RunError {
            kind,
            message,
            file,
            stderr: self.log.take_stderr(),
        });
    }
}

fn with_exit_status(message: String, status: Option<std::process::ExitStatus>) -> String {
    match status {
        Some(status) => format!("{message} (server {status})"),
        None => message,
    }
}
