//! The `--output jsonl` format: one JSON object per line on stdout.
//! The format is documented in `readme.md`.

use crate::cancel::ServerControl;
use serde::Serialize;
use slim_protocol::{ExceptionMessage, InstructionResultValue};
use std::io::Write;
use std::sync::{Arc, Mutex};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    Skipped,
    Snoozed,
    Failed,
    Error,
    Cancelled,
}

impl Status {
    fn severity(self) -> u8 {
        match self {
            Status::Passed => 0,
            Status::Skipped => 1,
            Status::Snoozed => 2,
            Status::Failed => 3,
            Status::Error => 4,
            Status::Cancelled => 5,
        }
    }

    pub fn worst(self, other: Status) -> Status {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }

    pub fn fails_the_run(self) -> bool {
        matches!(self, Status::Failed | Status::Error | Status::Cancelled)
    }
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

/// Row totals. Zero entries are left out of the JSON.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    #[serde(skip_serializing_if = "is_zero")]
    pub passed: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub failed: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub error: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub snoozed: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped: usize,
}

impl Counts {
    pub fn add_row(&mut self, status: Status) {
        match status {
            Status::Passed => self.passed += 1,
            Status::Failed => self.failed += 1,
            Status::Error => self.error += 1,
            Status::Snoozed => self.snoozed += 1,
            Status::Skipped | Status::Cancelled => self.skipped += 1,
        }
    }

    pub fn merge(&mut self, other: Counts) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.error += other.error;
        self.snoozed += other.snoozed;
        self.skipped += other.skipped;
    }
}

/// What the server returned for a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actual {
    Value {
        value: String,
    },
    Void,
    List {
        value: Vec<Actual>,
    },
    Exception {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    MissingSymbol {
        symbol: String,
    },
    ProtocolError {
        message: String,
    },
}

impl From<&InstructionResultValue> for Actual {
    fn from(value: &InstructionResultValue) -> Self {
        match value {
            InstructionResultValue::Ok => Actual::Value {
                value: "OK".to_string(),
            },
            InstructionResultValue::Void => Actual::Void,
            InstructionResultValue::String(value) => Actual::Value {
                value: value.clone(),
            },
            InstructionResultValue::List(values) => Actual::List {
                value: values.iter().map(Actual::from).collect(),
            },
            InstructionResultValue::Exception(message) => exception(message),
        }
    }
}

fn exception(exception: &ExceptionMessage) -> Actual {
    let raw = exception.raw_message();
    let message = exception.pretty_message().unwrap_or(raw).to_string();
    let detail = (message != raw).then(|| raw.to_string());
    Actual::Exception { message, detail }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Reset,
    Setter,
    Execute,
    Getter,
    Method,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub role: Role,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    pub status: Status,
    /// The source text of a setter's cell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The source text of a getter's cell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// What a `$SYMBOL` cell stood for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Actual>,
    /// The symbol a `$V=` cell assigned. `actual` holds the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<Actual>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Import,
    Make,
    BeginTable,
    EndTable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunErrorKind {
    ServerStartFailed,
    ServerDied,
    ProtocolError,
    NoMatch,
    BadConfig,
    ParseError,
    FileError,
    SuiteAborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableSummary {
    pub line: usize,
    pub class: String,
    pub columns: Vec<String>,
    pub rows: Vec<usize>,
    pub snoozed_until: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    RunStarted {
        schema: u32,
        temoc_version: &'static str,
    },
    FileStarted {
        file: String,
        tables: Vec<TableSummary>,
    },
    TableStarted {
        file: String,
        line: usize,
    },
    RowStarted {
        file: String,
        table: usize,
        line: usize,
    },
    RowFinished {
        file: String,
        table: usize,
        line: usize,
        status: Status,
        duration_ms: u128,
        checks: Vec<Check>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    TableFinished {
        file: String,
        line: usize,
        status: Status,
        duration_ms: u128,
        counts: Counts,
    },
    FileFinished {
        file: String,
        status: Status,
        duration_ms: u128,
        counts: Counts,
    },
    SetupFailed {
        file: String,
        line: usize,
        phase: Phase,
        status: Status,
        actual: Actual,
        #[serde(skip_serializing_if = "Option::is_none")]
        snoozed_until: Option<String>,
    },
    RunError {
        kind: RunErrorKind,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stderr: Option<String>,
    },
    Output {
        stream: OutputStream,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        table: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        row: Option<usize>,
    },
    RunFinished {
        status: Status,
        exit_code: i32,
        duration_ms: u128,
        counts: Counts,
    },
}

struct SinkState {
    writer: Box<dyn Write + Send>,
    closed: bool,
}

/// Writes events to stdout. Server output threads share it with the runner, so
/// each event is written and flushed under one lock.
pub struct EventSink {
    state: Mutex<SinkState>,
    control: Arc<ServerControl>,
}

impl EventSink {
    pub fn new(writer: Box<dyn Write + Send>, control: Arc<ServerControl>) -> Self {
        Self {
            state: Mutex::new(SinkState {
                writer,
                closed: false,
            }),
            control,
        }
    }

    pub fn emit(&self, event: &Event) {
        let mut state = self.state.lock().expect("event sink lock poisoned");
        if state.closed {
            return;
        }
        if write_line(&mut state.writer, event).is_err() {
            // Nobody is reading anymore, so there is no reason to keep testing.
            state.closed = true;
            self.control.cancel();
        }
    }

    /// Writes the last event of the run. Anything emitted later is dropped, so
    /// a late line of server output can't follow `run_finished`.
    pub fn finish(&self, event: &Event) {
        self.emit(event);
        self.state.lock().expect("event sink lock poisoned").closed = true;
    }
}

fn write_line(writer: &mut impl Write, event: &Event) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn status_takes_the_worst_of_two() {
        assert_eq!(Status::Failed, Status::Passed.worst(Status::Failed));
        assert_eq!(Status::Error, Status::Error.worst(Status::Failed));
        assert_eq!(Status::Snoozed, Status::Snoozed.worst(Status::Skipped));
        assert_eq!(Status::Skipped, Status::Skipped.worst(Status::Passed));
    }

    #[test]
    fn counts_leave_out_zeros() {
        let mut counts = Counts::default();
        counts.add_row(Status::Passed);
        assert_eq!(r#"{"passed":1}"#, serde_json::to_string(&counts).unwrap());
    }

    #[test]
    fn exceptions_split_the_message_from_the_raw_text() {
        let raw = "__stack__ message:<<boom>> more".to_string();
        assert_eq!(
            Actual::Exception {
                message: "boom".into(),
                detail: Some(raw.clone()),
            },
            Actual::from(&InstructionResultValue::Exception(ExceptionMessage::new(
                raw
            )))
        );
    }

    #[test]
    fn nested_lists_keep_their_shape() {
        assert_eq!(
            r#"{"kind":"list","value":[{"kind":"value","value":"a"},{"kind":"void"}]}"#,
            serde_json::to_string(&Actual::from(&InstructionResultValue::List(vec![
                InstructionResultValue::String("a".into()),
                InstructionResultValue::Void,
            ])))
            .unwrap()
        );
    }
}
