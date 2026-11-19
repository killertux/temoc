//! The `--output jsonl` run. `readme.md` documents what it prints.

use crate::app::{get_list_of_files, has_extension, split_selector};
use crate::cancel::{cancel_on_signals, ServerControl};
use crate::events::{Counts, Event, EventSink, RunErrorKind, Status, SCHEMA_VERSION};
use crate::processor::stream::{file_started, FileRunner, Halt};
use crate::processor::{load_file_plan, slim_plan::FilePlan, Filter};
use crate::server_pool::ServerPool;
use crate::servers::Servers;
use anyhow::Result;
use std::fs::metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub struct JsonlOptions {
    pub servers: Servers,
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
                server: None,
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
    pool: ServerPool,
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
        Self {
            pool: ServerPool::new(&options.servers, events, control),
            options,
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
        self.pool.finish();
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
        match load_file_plan(path, &filter, &self.options.servers) {
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
        self.events.emit(&file_started(file, &plan, &self.pool));
        self.pool.set_location(Some(file), None, None);
        self.pool.forget_stderr();

        let mut runner = FileRunner::new(file, self.events, self.control, &mut self.pool);
        let halt = runner.run(plan);
        let (status, counts) = (runner.status(), runner.counts());
        let problems = runner.take_problems();
        self.pool.finish_file();

        self.status = self.status.worst(status);
        self.counts.merge(counts);
        self.events.emit(&Event::FileFinished {
            file: file.to_string(),
            status,
            duration_ms: started.elapsed().as_millis(),
            counts,
        });
        for problem in problems {
            self.status = self.status.worst(Status::Error);
            self.events.emit(&Event::RunError {
                kind: problem.kind,
                message: problem.message,
                file: Some(file.to_string()),
                server: Some(problem.server),
                stderr: problem.stderr,
            });
        }
        if matches!(
            halt,
            Some(Halt::Connection { .. } | Halt::Suite(_) | Halt::Cancelled)
        ) {
            self.stopped = true;
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

    /// Reports a problem that no server is part of.
    fn run_error(&mut self, kind: RunErrorKind, message: String, file: Option<String>) {
        self.status = self.status.worst(Status::Error);
        self.events.emit(&Event::RunError {
            kind,
            message,
            file,
            server: None,
            stderr: None,
        });
    }
}
