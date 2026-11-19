//! Runs a file one batch at a time and reports every result as it arrives.

use super::markdown_commands::{Position, Snooze};
use super::slim_instructions_from_commands::{
    file_hooks, Batch, ExpectedResult, ExpectedResultValue, FilePlan, MethodCell, RowPlan,
    TablePlan,
};
use super::{foreign_symbol_message, symbol_name, State};
use crate::cancel::ServerControl;
use crate::events::{
    Actual, Check, Counts, Event, EventSink, Phase, Role, RunErrorKind, Status, TableSummary,
};
use crate::server_pool::{ServerFailure, ServerPool, Unavailable};
use slim_protocol::{
    Instruction, InstructionResult, InstructionResultValue, SendInstructionsError, SlimValue,
};
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

pub fn file_started(file: &str, plan: &FilePlan, pool: &ServerPool) -> Event {
    Event::FileStarted {
        file: file.to_string(),
        tables: plan
            .tables
            .iter()
            .map(|table| TableSummary {
                line: table.line,
                server: pool.name(table.server).to_string(),
                class: table.class.clone(),
                columns: table.columns.clone(),
                rows: table.rows.iter().map(|row| row.line).collect(),
                snoozed_until: table.snoozed.until(),
            })
            .collect(),
    }
}

/// How a file stands with a server it uses.
enum Standing {
    Ready,
    /// Its file hooks failed, so its tables are skipped.
    Blocked,
    /// It can't be used, so its tables fail with this message.
    Gone(String),
}

/// A server that a file has used.
struct Use {
    server: usize,
    standing: Standing,
    /// The `endFile` to call, once the hooks fixture was made.
    end: Option<Batch>,
}

pub struct FileRunner<'a> {
    file: &'a str,
    events: &'a EventSink,
    control: &'a ServerControl,
    pool: &'a mut ServerPool,
    state: State,
    tally: Tally,
    problems: Vec<ServerFailure>,
}

impl<'a> FileRunner<'a> {
    pub fn new(
        file: &'a str,
        events: &'a EventSink,
        control: &'a ServerControl,
        pool: &'a mut ServerPool,
    ) -> Self {
        Self {
            file,
            events,
            control,
            pool,
            state: State::default(),
            tally: Tally::default(),
            problems: Vec::new(),
        }
    }

    pub fn status(&self) -> Status {
        self.tally.status()
    }

    pub fn counts(&self) -> Counts {
        self.tally.counts
    }

    /// The servers that were lost and the suite aborts, to report after the
    /// file.
    pub fn take_problems(&mut self) -> Vec<ServerFailure> {
        std::mem::take(&mut self.problems)
    }

    /// Runs the tables of a file, each on its server. A server starts at the
    /// first table that needs it, which is also when its file hooks begin:
    /// the tables of a server don't run when its hooks fixture can't be made
    /// or `beginFile` fails, and `endFile` runs whenever the fixture was made
    /// and the server still answers.
    pub fn run(&mut self, plan: FilePlan) -> Option<Halt> {
        let mut uses: Vec<Use> = Vec::new();
        let mut halt = None;
        for table in plan.tables {
            if !table.selected || halt.is_some() {
                self.skip_table(table);
                continue;
            }
            let server = table.server;
            let index = match uses.iter().position(|used| used.server == server) {
                Some(index) => index,
                None => {
                    let (used, enter_halt) = self.enter(server);
                    halt = enter_halt;
                    uses.push(used);
                    uses.len() - 1
                }
            };
            match &uses[index].standing {
                _ if halt.is_some() => self.skip_table(table),
                Standing::Blocked => self.skip_table(table),
                Standing::Gone(message) => self.fail_table(table, message),
                Standing::Ready => {
                    let table_halt = self.run_table(table);
                    halt = self.settle(server, table_halt);
                    if let Some(message) = self.pool.gone(server) {
                        uses[index].standing = Standing::Gone(message.to_string());
                    }
                }
            }
        }
        for (server, batch) in plan.trailing {
            let ready = uses
                .iter()
                .any(|used| used.server == server && matches!(used.standing, Standing::Ready));
            if halt.is_some() || !ready {
                continue;
            }
            let mut tally = Tally::default();
            let (_, trailing_halt) = self.run_lifecycle(server, batch, &mut tally);
            self.tally.merge(&tally);
            halt = self.settle(server, trailing_halt);
        }
        for used in uses {
            let Some(end) = used.end else {
                continue;
            };
            if self.control.is_cancelled() || !self.pool.is_running(used.server) {
                continue;
            }
            let (_, end_halt) = self.run_hook(used.server, end);
            let end_halt = self.settle(used.server, end_halt);
            // A file that already stopped the run keeps its reason.
            halt = match halt {
                Some(Halt::Suite(_) | Halt::Cancelled | Halt::Connection { .. }) => halt,
                _ => end_halt.or(halt),
            };
        }
        halt
    }

    /// Starts using a server in this file: starts it if it isn't running,
    /// and begins its file hooks if it has any.
    fn enter(&mut self, server: usize) -> (Use, Option<Halt>) {
        let mut used = Use {
            server,
            standing: Standing::Ready,
            end: None,
        };
        match self.pool.start(server) {
            Ok(()) => {}
            Err(Unavailable::Cancelled) => {
                used.standing = Standing::Blocked;
                return (used, Some(Halt::Cancelled));
            }
            Err(Unavailable::Gone(message)) => {
                used.standing = Standing::Gone(message);
                return (used, None);
            }
            Err(Unavailable::Failed(failure)) => {
                used.standing = Standing::Gone(failure.message.clone());
                let halt = self.report(failure);
                return (used, halt);
            }
        }
        let Some(class) = self.pool.file_hooks(server).map(str::to_string) else {
            return (used, None);
        };
        let hooks = file_hooks(&class, self.file);
        let (made, make_halt) = self.run_hook(server, hooks.make);
        let mut halt = self.settle(server, make_halt);
        if made && halt.is_none() {
            used.end = Some(hooks.end);
            let (began, begin_halt) = self.run_hook(server, hooks.begin);
            halt = self.settle(server, begin_halt);
            if !began {
                used.standing = Standing::Blocked;
            }
        } else {
            used.standing = Standing::Blocked;
        }
        if let Some(message) = self.pool.gone(server) {
            used.standing = Standing::Gone(message.to_string());
        }
        (used, halt)
    }

    /// Decides what a stop means for the rest of the file. A lost connection
    /// only ends the tables of its server, unless there is no other server.
    fn settle(&mut self, server: usize, halt: Option<Halt>) -> Option<Halt> {
        match halt {
            Some(Halt::Connection { kind, message }) => {
                let failure = self.pool.lost(server, kind, message);
                self.report(failure)
            }
            Some(Halt::Suite(message)) => {
                self.problems.push(ServerFailure {
                    kind: RunErrorKind::SuiteAborted,
                    message: message.clone(),
                    server: self.pool.name(server).to_string(),
                    stderr: self.pool.take_stderr(server),
                });
                Some(Halt::Suite(message))
            }
            halt => halt,
        }
    }

    /// Records a server that can't be used anymore. With no other server,
    /// that stops the run.
    fn report(&mut self, failure: ServerFailure) -> Option<Halt> {
        let halt = self.pool.is_single().then(|| Halt::Connection {
            kind: failure.kind,
            message: failure.message.clone(),
        });
        self.tally.add_status(Status::Error);
        self.problems.push(failure);
        halt
    }

    /// Runs one part of the file hooks. Returns whether it passed, and why to
    /// stop, if that's the case.
    fn run_hook(&mut self, server: usize, batch: Batch) -> (bool, Option<Halt>) {
        self.pool.set_location(Some(self.file), None, None);
        let mut tally = Tally::default();
        let (_, halt) = self.run_lifecycle(server, batch, &mut tally);
        self.tally.merge(&tally);
        (tally.status() == Status::Passed, halt)
    }

    fn run_table(&mut self, table: TablePlan) -> Option<Halt> {
        let started = Instant::now();
        let line = table.line;
        let server = table.server;
        self.events.emit(&Event::TableStarted {
            file: self.file.to_string(),
            line,
            server: self.pool.name(server).to_string(),
        });
        self.pool.set_location(Some(self.file), Some(line), None);
        let mut tally = Tally::default();

        let (make_failed, mut halt) = self.run_lifecycle(server, table.setup, &mut tally);
        for row in table.rows {
            if halt.is_some() || make_failed {
                self.skip_row(server, line, row, &mut tally);
            } else {
                halt = self.run_row(server, line, row, &mut tally);
            }
        }
        if halt.is_none() && !make_failed {
            halt = self.run_lifecycle(server, table.teardown, &mut tally).1;
        }

        self.finish_table(server, line, started, &tally);
        halt.filter(|halt| *halt != Halt::Table)
    }

    fn skip_table(&mut self, table: TablePlan) {
        let started = Instant::now();
        let mut tally = Tally::default();
        for row in table.rows {
            self.skip_row(table.server, table.line, row, &mut tally);
        }
        self.finish_table(table.server, table.line, started, &tally);
    }

    /// Fails every row of a table whose server can't be used.
    fn fail_table(&mut self, table: TablePlan, message: &str) {
        let started = Instant::now();
        let mut tally = Tally::default();
        tally.add_status(Status::Error);
        for row in table.rows {
            let checks = skipped_checks(row.batch, &row.method_cells);
            self.finish_row(
                table.server,
                table.line,
                row.line,
                Instant::now(),
                Some(Status::Error),
                checks,
                Some(message.to_string()),
                &mut tally,
            );
        }
        self.finish_table(table.server, table.line, started, &tally);
    }

    fn finish_table(&mut self, server: usize, line: usize, started: Instant, tally: &Tally) {
        self.events.emit(&Event::TableFinished {
            file: self.file.to_string(),
            line,
            server: self.pool.name(server).to_string(),
            status: tally.status(),
            duration_ms: started.elapsed().as_millis(),
            counts: tally.counts,
        });
        self.tally.merge(tally);
    }

    /// Runs instructions that belong to no row. Returns whether a `Make`
    /// failed, and why to stop, if that's the case.
    fn run_lifecycle(
        &mut self,
        server: usize,
        batch: Batch,
        tally: &mut Tally,
    ) -> (bool, Option<Halt>) {
        let results = match self.exchange(server, &batch) {
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
                server: self.pool.name(server).to_string(),
                phase: phase(instruction),
                status: verdict.status,
                actual: verdict.actual,
                snoozed_until: snooze.until(),
            });
        }
        (make_failed, halt)
    }

    fn run_row(
        &mut self,
        server: usize,
        table: usize,
        row: RowPlan,
        tally: &mut Tally,
    ) -> Option<Halt> {
        let started = Instant::now();
        self.events.emit(&Event::RowStarted {
            file: self.file.to_string(),
            table,
            line: row.line,
            server: self.pool.name(server).to_string(),
        });
        self.pool
            .set_location(Some(self.file), Some(table), Some(row.line));
        self.state.on_server(self.pool.name(server));

        if row
            .batch
            .instructions
            .iter()
            .any(|instruction| self.unusable_symbol(instruction).is_some())
        {
            let checks = self.checks_for_unusable_symbols(row.batch, &row.method_cells);
            self.finish_row(server, table, row.line, started, None, checks, None, tally);
            return None;
        }

        let results = match self.exchange(server, &row.batch) {
            Ok(results) => results,
            Err(halt) => {
                let checks = skipped_checks(row.batch, &row.method_cells);
                let (status, message) = match &halt {
                    Halt::Connection { message, .. } => {
                        (Some(Status::Error), Some(message.clone()))
                    }
                    _ => (Some(Status::Skipped), None),
                };
                self.finish_row(
                    server, table, row.line, started, status, checks, message, tally,
                );
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
        self.finish_row(server, table, row.line, started, None, checks, None, tally);
        halt
    }

    fn skip_row(&mut self, server: usize, table: usize, row: RowPlan, tally: &mut Tally) {
        let checks = skipped_checks(row.batch, &row.method_cells);
        self.finish_row(
            server,
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
        server: usize,
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
            server: self.pool.name(server).to_string(),
            status,
            duration_ms: started.elapsed().as_millis(),
            checks,
            message,
        });
    }

    /// Sends a batch to a server. A shorter answer is only valid after a
    /// control exception.
    fn exchange(&mut self, server: usize, batch: &Batch) -> Result<Vec<InstructionResult>, Halt> {
        if self.control.is_cancelled() {
            return Err(Halt::Cancelled);
        }
        // `FitNesse` never sends an empty list either.
        if batch.instructions.is_empty() {
            return Ok(Vec::new());
        }
        self.state.on_server(self.pool.name(server));
        let control = self.control;
        let Some(connection) = self.pool.connection(server) else {
            return Err(Halt::Connection {
                kind: RunErrorKind::ServerDied,
                message: "The server is not running".to_string(),
            });
        };
        let results = connection
            .send_instructions(&batch.instructions)
            .map_err(|error| connection_halt(control, &error))?;
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
                let set_on = self.state.foreign_server(symbol).map(str::to_string);
                return Verdict {
                    status: under_snooze(Status::Error, snooze),
                    actual: self.unusable(symbol.clone(), set_on, &expected.position),
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

    /// What a cell gets for a symbol that the server in use doesn't have:
    /// one that was never set, or one that another server keeps.
    fn unusable(&self, symbol: String, set_on: Option<String>, position: &Position) -> Actual {
        match set_on {
            None => Actual::MissingSymbol { symbol },
            Some(server) => Actual::SymbolFromOtherServer {
                message: foreign_symbol_message(
                    &symbol,
                    &server,
                    self.state.server(),
                    self.file,
                    position,
                ),
                symbol,
                server,
            },
        }
    }

    /// The symbol of an input cell that the server in use doesn't have, and
    /// the server that set it, if one did. The server would pass an unknown
    /// `$NAME` on as literal text, and a fixture that takes strings would
    /// then quietly test the wrong thing.
    fn unusable_symbol(&self, instruction: &Instruction) -> Option<(String, Option<String>)> {
        let Instruction::Call { args, .. } = instruction else {
            return None;
        };
        args.iter().find_map(|arg| {
            let SlimValue::String(text) = arg else {
                return None;
            };
            let name = symbol_name(text).filter(|name| self.state.get_symbol(name).is_none())?;
            let set_on = self.state.foreign_server(name).map(str::to_string);
            Some((name.to_string(), set_on))
        })
    }

    fn checks_for_unusable_symbols(&self, batch: Batch, cells: &[MethodCell]) -> Vec<Check> {
        let mut checks = Vec::new();
        let items = batch.instructions.iter().zip(batch.expected_result);
        for (instruction, (expected, snooze)) in items {
            let mut check = describe(instruction, &expected, cells);
            match self.unusable_symbol(instruction) {
                Some((symbol, set_on)) => {
                    check.status = under_snooze(Status::Error, &snooze);
                    check.actual = Some(self.unusable(symbol, set_on, &expected.position));
                    check.snoozed_until = snooze.until();
                    checks.push(check);
                }
                None => push_check(&mut checks, skipped(check)),
            }
        }
        checks
    }
}

fn connection_halt(control: &ServerControl, error: &SendInstructionsError) -> Halt {
    if control.is_cancelled() {
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

fn phase(instruction: &Instruction) -> Phase {
    match instruction {
        Instruction::Import { .. } => Phase::Import,
        Instruction::Make { .. } => Phase::Make,
        Instruction::Call { function, .. } if function == "beginTable" => Phase::BeginTable,
        Instruction::Call { function, .. } if function == "beginFile" => Phase::BeginFile,
        Instruction::Call { function, .. } if function == "endFile" => Phase::EndFile,
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
