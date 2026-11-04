//! Runs a file one batch at a time and reports every result as it arrives.

use super::markdown_commands::Snooze;
use super::slim_instructions_from_commands::{
    Batch, ExpectedResult, ExpectedResultValue, FilePlan, MethodCell, RowPlan, TablePlan,
};
use super::State;
use crate::cancel::ServerControl;
use crate::captured_server::ServerLog;
use crate::events::{
    Actual, Check, Counts, Event, EventSink, Phase, Role, RunErrorKind, Status, TableSummary,
};
use slim_protocol::{
    Instruction, InstructionResult, InstructionResultValue, SendInstructionsError, SlimConnection,
    SlimValue,
};
use std::io::{Read, Write};
use std::time::Instant;

/// Why the instructions after some point were not sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt {
    /// `IGNORE_SCRIPT_TEST`: the next table still runs.
    Table,
    /// `ABORT_SLIM_TEST` or `IGNORE_ALL_TESTS`: the next file still runs.
    File,
    /// `ABORT_SLIM_SUITE`: nothing else runs.
    Suite(String),
    Cancelled,
    /// The connection can't be used anymore.
    Connection {
        kind: RunErrorKind,
        message: String,
    },
}

/// A control exception ends the batch on the server, and nothing stops the
/// next batch but the client. The tags are matched like `FitNesse` does, which
/// is anywhere in the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    AbortTest,
    AbortSuite,
    IgnoreScript,
    IgnoreAll,
}

impl Control {
    fn of(value: &InstructionResultValue) -> Option<Self> {
        let InstructionResultValue::Exception(message) = value else {
            return None;
        };
        [
            ("ABORT_SLIM_TEST", Control::AbortTest),
            ("ABORT_SLIM_SUITE", Control::AbortSuite),
            ("IGNORE_SCRIPT_TEST", Control::IgnoreScript),
            ("IGNORE_ALL_TESTS", Control::IgnoreAll),
        ]
        .into_iter()
        .find_map(|(tag, control)| message.raw_message().contains(tag).then_some(control))
    }

    fn status(self, snooze: &Snooze) -> Status {
        match self {
            Control::AbortTest | Control::AbortSuite => under_snooze(Status::Error, snooze),
            Control::IgnoreScript | Control::IgnoreAll => Status::Skipped,
        }
    }

    fn halt(self, value: &InstructionResultValue) -> Halt {
        match self {
            Control::IgnoreScript => Halt::Table,
            Control::AbortTest | Control::IgnoreAll => Halt::File,
            Control::AbortSuite => Halt::Suite(value.to_string()),
        }
    }
}

fn under_snooze(status: Status, snooze: &Snooze) -> Status {
    if snooze.should_snooze() {
        Status::Snoozed
    } else {
        status
    }
}

struct Verdict {
    status: Status,
    actual: Actual,
    resolved: Option<Actual>,
    halt: Option<Halt>,
}

#[derive(Default)]
struct Tally {
    status: Option<Status>,
    counts: Counts,
}

impl Tally {
    fn add_status(&mut self, status: Status) {
        self.status = Some(self.status().worst(status));
    }

    fn add_row(&mut self, status: Status) {
        self.add_status(status);
        self.counts.add_row(status);
    }

    fn merge(&mut self, other: &Tally) {
        self.add_status(other.status());
        self.counts.merge(other.counts);
    }

    fn status(&self) -> Status {
        self.status.unwrap_or(Status::Passed)
    }
}

pub fn file_started(file: &str, plan: &FilePlan) -> Event {
    Event::FileStarted {
        file: file.to_string(),
        tables: plan
            .tables
            .iter()
            .map(|table| TableSummary {
                line: table.line,
                class: table.class.clone(),
                columns: table.columns.clone(),
                rows: table.rows.iter().map(|row| row.line).collect(),
                snoozed_until: table.snoozed.until(),
            })
            .collect(),
    }
}

pub struct FileRunner<'a> {
    file: &'a str,
    events: &'a EventSink,
    log: &'a ServerLog,
    control: &'a ServerControl,
    state: State,
    tally: Tally,
}

impl<'a> FileRunner<'a> {
    pub fn new(
        file: &'a str,
        events: &'a EventSink,
        log: &'a ServerLog,
        control: &'a ServerControl,
    ) -> Self {
        Self {
            file,
            events,
            log,
            control,
            state: State::default(),
            tally: Tally::default(),
        }
    }

    pub fn status(&self) -> Status {
        self.tally.status()
    }

    pub fn counts(&self) -> Counts {
        self.tally.counts
    }

    pub fn mark_error(&mut self) {
        self.tally.add_status(Status::Error);
    }

    /// Reports every row as skipped when the server never came up.
    pub fn skip(&mut self, plan: FilePlan) {
        for table in plan.tables {
            self.skip_table(table);
        }
    }

    pub fn run<R: Read, W: Write>(
        &mut self,
        connection: &mut SlimConnection<R, W>,
        plan: FilePlan,
    ) -> Option<Halt> {
        let mut halt = None;
        for table in plan.tables {
            if halt.is_some() {
                self.skip_table(table);
            } else {
                halt = self.run_table(connection, table);
            }
        }
        if halt.is_none() {
            let mut tally = Tally::default();
            halt = self.run_lifecycle(connection, plan.trailing, &mut tally).1;
            self.tally.merge(&tally);
        }
        halt
    }

    fn run_table<R: Read, W: Write>(
        &mut self,
        connection: &mut SlimConnection<R, W>,
        table: TablePlan,
    ) -> Option<Halt> {
        let started = Instant::now();
        let line = table.line;
        self.events.emit(&Event::TableStarted {
            file: self.file.to_string(),
            line,
        });
        self.log.set_location(Some(self.file), Some(line), None);
        let mut tally = Tally::default();

        let (make_failed, mut halt) = self.run_lifecycle(connection, table.setup, &mut tally);
        for row in table.rows {
            if halt.is_some() || make_failed {
                self.skip_row(line, row, &mut tally);
            } else {
                halt = self.run_row(connection, line, row, &mut tally);
            }
        }
        if halt.is_none() && !make_failed {
            halt = self.run_lifecycle(connection, table.teardown, &mut tally).1;
        }

        self.finish_table(line, started, &tally);
        halt.filter(|halt| *halt != Halt::Table)
    }

    fn skip_table(&mut self, table: TablePlan) {
        let started = Instant::now();
        let mut tally = Tally::default();
        for row in table.rows {
            self.skip_row(table.line, row, &mut tally);
        }
        self.finish_table(table.line, started, &tally);
    }

    fn finish_table(&mut self, line: usize, started: Instant, tally: &Tally) {
        self.events.emit(&Event::TableFinished {
            file: self.file.to_string(),
            line,
            status: tally.status(),
            duration_ms: started.elapsed().as_millis(),
            counts: tally.counts,
        });
        self.tally.merge(tally);
    }

    /// Runs instructions that belong to no row. Returns whether a `Make`
    /// failed, and why to stop, if that's the case.
    fn run_lifecycle<R: Read, W: Write>(
        &mut self,
        connection: &mut SlimConnection<R, W>,
        batch: Batch,
        tally: &mut Tally,
    ) -> (bool, Option<Halt>) {
        let results = match self.exchange(connection, &batch) {
            Ok(results) => results,
            Err(halt) => {
                if matches!(halt, Halt::Connection { .. }) {
                    tally.add_status(Status::Error);
                }
                return (false, Some(halt));
            }
        };
        let mut make_failed = false;
        let mut halt = None;
        let items = batch.instructions.iter().zip(batch.expected_result);
        for ((instruction, (expected, snooze)), result) in items.zip(results) {
            // Without an instance, `beginTable` can only repeat the same failure.
            if make_failed {
                break;
            }
            let line = expected.position.line();
            let verdict = self.judge(expected, &snooze, &result);
            halt = halt.or(verdict.halt);
            if verdict.status == Status::Passed {
                continue;
            }
            tally.add_status(verdict.status);
            make_failed |= matches!(instruction, Instruction::Make { .. });
            self.events.emit(&Event::SetupFailed {
                file: self.file.to_string(),
                line,
                phase: phase(instruction),
                status: verdict.status,
                actual: verdict.actual,
                snoozed_until: snooze.until(),
            });
        }
        (make_failed, halt)
    }

    fn run_row<R: Read, W: Write>(
        &mut self,
        connection: &mut SlimConnection<R, W>,
        table: usize,
        row: RowPlan,
        tally: &mut Tally,
    ) -> Option<Halt> {
        let started = Instant::now();
        self.events.emit(&Event::RowStarted {
            file: self.file.to_string(),
            table,
            line: row.line,
        });
        self.log
            .set_location(Some(self.file), Some(table), Some(row.line));

        if row
            .batch
            .instructions
            .iter()
            .any(|instruction| self.missing_symbol(instruction).is_some())
        {
            let checks = self.checks_for_missing_symbols(row.batch, &row.method_cells);
            self.finish_row(table, row.line, started, None, checks, None, tally);
            return None;
        }

        let results = match self.exchange(connection, &row.batch) {
            Ok(results) => results,
            Err(halt) => {
                let checks = skipped_checks(row.batch, &row.method_cells);
                let (status, message) = match &halt {
                    Halt::Connection { message, .. } => {
                        (Some(Status::Error), Some(message.clone()))
                    }
                    _ => (Some(Status::Skipped), None),
                };
                self.finish_row(table, row.line, started, status, checks, message, tally);
                return Some(halt);
            }
        };

        let mut checks = Vec::new();
        let mut halt = None;
        let mut results = results.into_iter();
        let items = row.batch.instructions.iter().zip(row.batch.expected_result);
        for (instruction, (expected, snooze)) in items {
            let check = describe(instruction, &expected, &row.method_cells);
            let Some(result) = results.next() else {
                push_check(&mut checks, skipped(check));
                continue;
            };
            if let Some(cells) = per_cell_values(&check, &row.method_cells, &result) {
                checks.extend(method_cell_checks(&check, cells, &snooze));
                continue;
            }
            let verdict = self.judge(expected, &snooze, &result);
            halt = halt.or(verdict.halt.clone());
            push_check(
                &mut checks,
                with_verdict(check, verdict, &snooze, &self.state),
            );
        }
        self.finish_row(table, row.line, started, None, checks, None, tally);
        halt
    }

    fn skip_row(&mut self, table: usize, row: RowPlan, tally: &mut Tally) {
        let checks = skipped_checks(row.batch, &row.method_cells);
        self.finish_row(
            table,
            row.line,
            Instant::now(),
            Some(Status::Skipped),
            checks,
            None,
            tally,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_row(
        &self,
        table: usize,
        line: usize,
        started: Instant,
        status: Option<Status>,
        checks: Vec<Check>,
        message: Option<String>,
        tally: &mut Tally,
    ) {
        let status = status.unwrap_or_else(|| {
            checks
                .iter()
                .fold(Status::Passed, |status, check| status.worst(check.status))
        });
        tally.add_row(status);
        self.events.emit(&Event::RowFinished {
            file: self.file.to_string(),
            table,
            line,
            status,
            duration_ms: started.elapsed().as_millis(),
            checks,
            message,
        });
    }

    /// Sends a batch. A shorter answer is only valid after a control exception.
    fn exchange<R: Read, W: Write>(
        &self,
        connection: &mut SlimConnection<R, W>,
        batch: &Batch,
    ) -> Result<Vec<InstructionResult>, Halt> {
        if self.control.is_cancelled() {
            return Err(Halt::Cancelled);
        }
        // `FitNesse` never sends an empty list either.
        if batch.instructions.is_empty() {
            return Ok(Vec::new());
        }
        let results = connection
            .send_instructions(&batch.instructions)
            .map_err(|error| self.connection_halt(&error))?;
        let stopped_early = results
            .last()
            .is_some_and(|result| Control::of(&result.value).is_some());
        let expected = batch.instructions.len();
        if results.len() > expected || (results.len() < expected && !stopped_early) {
            return Err(Halt::Connection {
                kind: RunErrorKind::ProtocolError,
                message: format!(
                    "Number of instruction results `{}` does not matched the expected number of results `{expected}`",
                    results.len()
                ),
            });
        }
        Ok(results)
    }

    fn connection_halt(&self, error: &SendInstructionsError) -> Halt {
        if self.control.is_cancelled() {
            return Halt::Cancelled;
        }
        let kind = match error {
            SendInstructionsError::IoError(_)
            | SendInstructionsError::FromSlimReaderError(
                slim_protocol::FromSlimReaderError::IoError(_),
            ) => RunErrorKind::ServerDied,
            SendInstructionsError::FromSlimReaderError(_) => RunErrorKind::ProtocolError,
        };
        Halt::Connection {
            kind,
            message: error.to_string(),
        }
    }

    fn judge(
        &mut self,
        mut expected: ExpectedResult,
        snooze: &Snooze,
        result: &InstructionResult,
    ) -> Verdict {
        let actual = Actual::from(&result.value);
        if expected.id != result.id {
            return Verdict {
                status: under_snooze(Status::Error, snooze),
                actual: Actual::ProtocolError {
                    message: format!(
                        "Different ID in response. Expected {} but got {}",
                        expected.id, result.id
                    ),
                },
                resolved: None,
                halt: None,
            };
        }
        if let Some(control) = Control::of(&result.value) {
            return Verdict {
                status: control.status(snooze),
                actual,
                resolved: None,
                halt: Some(control.halt(&result.value)),
            };
        }
        let mut resolved = None;
        if let ExpectedResultValue::Symbol(symbol) = &expected.value {
            let Some(value) = self.state.get_symbol(symbol).cloned() else {
                return Verdict {
                    status: under_snooze(Status::Error, snooze),
                    actual: Actual::MissingSymbol {
                        symbol: symbol.clone(),
                    },
                    resolved: None,
                    halt: None,
                };
            };
            resolved = Some(Actual::from(&value));
            expected.value = ExpectedResultValue::Result(value);
        }
        let is_exception = matches!(result.value, InstructionResultValue::Exception(_));
        let status = if expected.value == result.value {
            Status::Passed
        } else if is_exception {
            under_snooze(Status::Error, snooze)
        } else {
            under_snooze(Status::Failed, snooze)
        };
        if let ExpectedResultValue::SetSymbol(symbol) = expected.value {
            if !is_exception {
                self.state.set_symbol(symbol, result.value.clone());
            }
        }
        Verdict {
            status,
            actual,
            resolved,
            halt: None,
        }
    }

    /// The server would pass an unknown `$NAME` on as literal text, and a
    /// fixture that takes strings would then quietly test the wrong thing.
    fn missing_symbol(&self, instruction: &Instruction) -> Option<String> {
        let Instruction::Call { args, .. } = instruction else {
            return None;
        };
        args.iter().find_map(|arg| {
            let SlimValue::String(text) = arg else {
                return None;
            };
            symbol_name(text)
                .filter(|name| self.state.get_symbol(name).is_none())
                .map(str::to_string)
        })
    }

    fn checks_for_missing_symbols(&self, batch: Batch, cells: &[MethodCell]) -> Vec<Check> {
        let mut checks = Vec::new();
        let items = batch.instructions.iter().zip(batch.expected_result);
        for (instruction, (expected, snooze)) in items {
            let mut check = describe(instruction, &expected, cells);
            match self.missing_symbol(instruction) {
                Some(symbol) => {
                    check.status = under_snooze(Status::Error, &snooze);
                    check.actual = Some(Actual::MissingSymbol { symbol });
                    check.snoozed_until = snooze.until();
                    checks.push(check);
                }
                None => push_check(&mut checks, skipped(check)),
            }
        }
        checks
    }
}

fn symbol_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix('$')?;
    let mut chars = name.chars();
    let valid = chars.next().is_some_and(char::is_alphabetic)
        && chars.all(|c| c.is_alphanumeric() || c == '_');
    valid.then_some(name)
}

fn phase(instruction: &Instruction) -> Phase {
    match instruction {
        Instruction::Import { .. } => Phase::Import,
        Instruction::Make { .. } => Phase::Make,
        Instruction::Call { function, .. } if function == "beginTable" => Phase::BeginTable,
        _ => Phase::EndTable,
    }
}

/// Builds the part of a check that doesn't depend on the result.
fn describe(instruction: &Instruction, expected: &ExpectedResult, cells: &[MethodCell]) -> Check {
    let (function, args) = match instruction {
        Instruction::Call { function, args, .. }
        | Instruction::CallAndAssign { function, args, .. } => (function.as_str(), args.as_slice()),
        _ => ("", [].as_slice()),
    };
    let role = match &expected.value {
        ExpectedResultValue::NullOrVoid => Role::Setter,
        ExpectedResultValue::NullOrVoidOrMethodNotFound if function == "reset" => Role::Reset,
        ExpectedResultValue::NullOrVoidOrMethodNotFound => Role::Execute,
        ExpectedResultValue::List(_) => Role::Method,
        _ => Role::Getter,
    };
    let source = match &expected.value {
        ExpectedResultValue::String(value) => Some(value.clone()),
        ExpectedResultValue::Symbol(symbol) => Some(format!("${symbol}")),
        ExpectedResultValue::SetSymbol(symbol) => Some(format!("${symbol}=")),
        ExpectedResultValue::List(_) => Some(
            cells
                .iter()
                .map(|cell| cell.expected.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    };
    let assigned = match &expected.value {
        ExpectedResultValue::SetSymbol(symbol) => Some(symbol.clone()),
        _ => None,
    };
    let value = match (role, args) {
        (Role::Setter, [SlimValue::String(value)]) => Some(value.clone()),
        _ => None,
    };
    Check {
        role,
        method: function.to_string(),
        column: matches!(role, Role::Setter | Role::Getter).then(|| expected.position.column()),
        status: Status::Passed,
        value,
        expected: source,
        resolved: None,
        assigned,
        actual: None,
        snoozed_until: None,
    }
}

/// `reset` and `execute` are optional, so they only show up when they fail.
fn push_check(checks: &mut Vec<Check>, check: Check) {
    let lifecycle = matches!(check.role, Role::Reset | Role::Execute);
    if !lifecycle
        || matches!(
            check.status,
            Status::Failed | Status::Error | Status::Snoozed
        )
    {
        checks.push(check);
    }
}

fn skipped(mut check: Check) -> Check {
    check.status = Status::Skipped;
    check
}

fn skipped_checks(batch: Batch, cells: &[MethodCell]) -> Vec<Check> {
    let mut checks = Vec::new();
    let items = batch.instructions.iter().zip(batch.expected_result);
    for (instruction, (expected, _)) in items {
        push_check(
            &mut checks,
            skipped(describe(instruction, &expected, cells)),
        );
    }
    checks
}

fn with_verdict(mut check: Check, verdict: Verdict, snooze: &Snooze, state: &State) -> Check {
    let passed = verdict.status == Status::Passed;
    check.status = verdict.status;
    check.resolved = verdict.resolved;
    if check.role == Role::Setter {
        check.resolved = check
            .value
            .as_deref()
            .and_then(symbol_name)
            .and_then(|name| state.get_symbol(name))
            .map(Actual::from);
    }
    if !passed || check.role != Role::Setter {
        check.actual = Some(verdict.actual);
    }
    if !passed {
        check.snoozed_until = snooze.until();
    }
    check
}

/// The list to compare cell by cell, when a `#method` row returned one with an
/// element for each cell. Anything else is judged as a whole, with no column.
fn per_cell_values<'r>(
    check: &Check,
    cells: &'r [MethodCell],
    result: &'r InstructionResult,
) -> Option<Vec<(&'r MethodCell, &'r InstructionResultValue)>> {
    let InstructionResultValue::List(values) = &result.value else {
        return None;
    };
    (check.role == Role::Method && values.len() == cells.len())
        .then(|| cells.iter().zip(values).collect())
}

fn method_cell_checks(
    check: &Check,
    cells: Vec<(&MethodCell, &InstructionResultValue)>,
    snooze: &Snooze,
) -> Vec<Check> {
    cells
        .into_iter()
        .map(|(cell, value)| {
            let status = if ExpectedResultValue::String(cell.expected.clone()) == *value {
                Status::Passed
            } else if matches!(value, InstructionResultValue::Exception(_)) {
                under_snooze(Status::Error, snooze)
            } else {
                under_snooze(Status::Failed, snooze)
            };
            Check {
                column: Some(cell.column),
                status,
                expected: Some(cell.expected.clone()),
                actual: Some(Actual::from(value)),
                snoozed_until: (status != Status::Passed).then(|| snooze.until()).flatten(),
                ..check.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use slim_protocol::ExceptionMessage;

    fn exception(text: &str) -> InstructionResultValue {
        InstructionResultValue::Exception(ExceptionMessage::new(text.into()))
    }

    #[test]
    fn control_tags_are_found_with_or_without_a_message() {
        for (text, control) in [
            ("ABORT_SLIM_TEST:", Control::AbortTest),
            ("ABORT_SLIM_SUITE:message:<<stop>>", Control::AbortSuite),
            ("IGNORE_SCRIPT_TEST:", Control::IgnoreScript),
            ("IGNORE_ALL_TESTS:message:<<no>>", Control::IgnoreAll),
        ] {
            assert_eq!(Some(control), Control::of(&exception(text)));
        }
        assert_eq!(None, Control::of(&exception("message:<<boom>>")));
        assert_eq!(
            None,
            Control::of(&InstructionResultValue::String("ABORT_SLIM_TEST".into()))
        );
    }

    #[test]
    fn symbol_names_are_whole_cells_that_start_with_a_letter() {
        assert_eq!(Some("V"), symbol_name("$V"));
        assert_eq!(Some("my_value2"), symbol_name("$my_value2"));
        assert_eq!(None, symbol_name("$5.00"));
        assert_eq!(None, symbol_name("$"));
        assert_eq!(None, symbol_name("costs $V"));
    }
}
