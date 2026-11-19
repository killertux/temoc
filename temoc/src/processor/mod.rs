pub use self::markdown_commands::Snooze;
pub use self::slim_instructions_from_commands::{file_hooks, Batch, Segment};
use self::{
    markdown_commands::{get_commands_from_markdown, Position},
    slim_instructions_from_commands::{
        get_file_plan, get_segments, ExpectedResultValue, FilePlan, Placed,
    },
};
use crate::processor::markdown_commands::MarkdownCommand;
use crate::processor::validate_result::validate_result;
use crate::servers::Servers;
use anyhow::{anyhow, bail, Result};
use markdown::mdast::Node;
use regex::Regex;
use slim_protocol::{Instruction, InstructionResultValue, SlimConnection, SlimValue};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::{
    fs::read_to_string,
    io::{Read, Write},
    path::Path,
};

mod markdown_commands;
mod slim_instructions_from_commands;
pub mod stream;
pub mod slim_plan {
    pub use super::slim_instructions_from_commands::FilePlan;
}
mod validate_result;

/// The symbols of a file. Each server keeps its own, so a symbol is only
/// known on the server that set it.
#[derive(Debug, Default)]
pub struct State {
    /// For each symbol, its value on every server that set it.
    symbols: HashMap<String, Vec<(String, InstructionResultValue)>>,
    /// The server in use.
    server: String,
    name_server: bool,
}

impl State {
    /// `name_server` adds the server to the failures of a file that uses
    /// several.
    pub fn new(name_server: bool) -> Self {
        Self {
            name_server,
            ..Self::default()
        }
    }

    /// Changes the server that symbols are set on and read from.
    pub fn on_server(&mut self, server: &str) {
        if self.server != server {
            self.server = server.to_string();
        }
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    pub fn set_symbol(&mut self, key: String, value: InstructionResultValue) {
        let values = self.symbols.entry(key).or_default();
        values.retain(|(server, _)| *server != self.server);
        values.push((self.server.clone(), value));
    }

    pub fn get_symbol(&self, key: &str) -> Option<&InstructionResultValue> {
        self.symbols
            .get(key)?
            .iter()
            .find(|(server, _)| *server == self.server)
            .map(|(_, value)| value)
    }

    /// The server that set a symbol, when the one in use didn't.
    pub fn foreign_server(&self, key: &str) -> Option<&str> {
        if self.get_symbol(key).is_some() {
            return None;
        }
        let (server, _) = self.symbols.get(key)?.last()?;
        Some(server)
    }

    /// What ends a failure message, to tell the servers of a file apart.
    pub fn server_suffix(&self) -> String {
        if self.name_server {
            format!(" on server {}", self.server)
        } else {
            String::new()
        }
    }
}

pub fn foreign_symbol_message(
    symbol: &str,
    set_on: &str,
    used_on: &str,
    file_path: impl Display,
    position: &Position,
) -> String {
    format!(
        "Symbol `${symbol}` was set on server `{set_on}` and cannot be used on server `{used_on}` in {file_path}:{position}"
    )
}

/// The name in a cell that is exactly `$NAME`.
pub fn symbol_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix('$')?;
    let mut chars = name.chars();
    let valid = chars.next().is_some_and(char::is_alphabetic)
        && chars.all(|c| c.is_alphanumeric() || c == '_');
    valid.then_some(name)
}

#[derive(Debug, Clone)]
pub struct Filter {
    filters: Vec<FilterType>,
}

#[derive(Debug, Clone)]
enum FilterType {
    FixtureClass(Regex),
    Line(usize),
}

impl Filter {
    pub fn new() -> Self {
        Self { filters: vec![] }
    }

    pub fn fixture_class(mut self, fixture: &str) -> Result<Self> {
        self.filters
            .push(FilterType::FixtureClass(Regex::new(fixture)?));
        Ok(self)
    }

    pub fn line(mut self, line: usize) -> Self {
        self.filters.push(FilterType::Line(line));
        self
    }

    pub fn apply(&self, commands: Vec<MarkdownCommand>) -> Vec<MarkdownCommand> {
        let mut commands: Vec<MarkdownCommand> = commands
            .into_iter()
            .filter(|command| {
                self.filters.iter().all(|filter| match filter {
                    FilterType::FixtureClass(regex) => match command {
                        MarkdownCommand::DecisionTable { class, .. } => regex.is_match(&class.0),
                        MarkdownCommand::Import { .. } | MarkdownCommand::Server { .. } => true,
                    },
                    FilterType::Line(line) => match command {
                        MarkdownCommand::DecisionTable { class, table, .. } => {
                            class.1.line() == *line
                                || table.iter().any(|row| row.position.line() == *line)
                        }
                        MarkdownCommand::Import { .. } | MarkdownCommand::Server { .. } => true,
                    },
                })
            })
            .collect();
        for filter in &self.filters {
            if let FilterType::Line(line) = filter {
                commands.iter_mut().for_each(|command| {
                    keep_only_selected_row(command, *line);
                });
            }
        }
        commands
    }
}

/// A selector on a row runs just that row, while one on the directive runs
/// the whole table.
fn keep_only_selected_row(command: &mut MarkdownCommand, line: usize) {
    if let MarkdownCommand::DecisionTable { class, table, .. } = command {
        if class.1.line() != line {
            table.retain(|row| row.position.line() == line);
        }
    }
}

fn is_table(command: &MarkdownCommand) -> bool {
    matches!(command, MarkdownCommand::DecisionTable { .. })
}

/// Reads a file into what it sends to each server, in order. Empty when no
/// table is left to run.
pub fn load_file_segments(
    file_path: impl AsRef<Path>,
    filter: &Filter,
    servers: &Servers,
) -> Result<Vec<Segment>> {
    let file_path = file_path.as_ref();
    print!("Testing file {}...", file_path.display());
    let mut commands = load_commands(file_path, filter, servers)?;
    commands.retain(|(server, command)| !is_table(command) || servers.is_selected(*server));
    if !commands.iter().any(|(_, command)| is_table(command)) {
        return Ok(Vec::new());
    }
    Ok(get_segments(commands))
}

fn load_commands(file_path: &Path, filter: &Filter, servers: &Servers) -> Result<Vec<Placed>> {
    let markdown = parse_markdown(file_path)?;
    let file = file_path.display().to_string();
    let commands = filter.apply(get_commands_from_markdown(markdown, &file)?);
    let mut commands = place_on_servers(commands, servers, &file)?;
    drop_idle_imports(&mut commands, servers);
    Ok(commands)
}

/// Gives each command the server that the last `server` directive above it
/// names, or the default one.
fn place_on_servers(
    commands: Vec<MarkdownCommand>,
    servers: &Servers,
    file: &str,
) -> Result<Vec<Placed>> {
    let mut current = servers.default_index();
    let mut placed = Vec::new();
    for command in commands {
        if let MarkdownCommand::Server { name, position } = &command {
            let Some(index) = servers.index_of(name) else {
                bail!(
                    "Unknown server `{name}` at {file}:{position}. The configured servers are: {}",
                    servers.names()
                )
            };
            current = index;
            continue;
        }
        placed.push((current, command));
    }
    Ok(placed)
}

/// An import alone must not start a server, so the imports of a server go
/// when no table of the file runs on it.
fn drop_idle_imports(commands: &mut Vec<Placed>, servers: &Servers) {
    let used: HashSet<usize> = commands
        .iter()
        .filter(|(server, command)| is_table(command) && servers.is_selected(*server))
        .map(|(server, _)| *server)
        .collect();
    commands.retain(|(server, command)| is_table(command) || used.contains(server));
}

/// Reads a file into the batches that run it. `None` when no table matches.
pub fn load_file_plan(
    file_path: &Path,
    filter: &Filter,
    servers: &Servers,
) -> Result<Option<FilePlan>> {
    let commands = load_commands(file_path, filter, servers)?;
    if !commands.iter().any(|(_, command)| is_table(command)) {
        return Ok(None);
    }
    Ok(Some(get_file_plan(commands, servers)))
}

/// What went wrong, and the snooze it is under.
pub type Failures = Vec<(String, Snooze)>;

/// Sends one batch and returns what went wrong in it.
pub fn execute_instructions<R: Read, W: Write>(
    connection: &mut SlimConnection<R, W>,
    file_path: &str,
    mut batch: Batch,
    state: &mut State,
) -> Result<Failures> {
    let mut failures = fail_foreign_symbols(file_path, &mut batch, state);
    let result = connection.send_instructions(&batch.instructions)?;
    failures.extend(validate_result(
        file_path,
        batch.expected_result,
        result,
        state,
    )?);
    Ok(failures)
}

/// Fails the input cells that use a symbol of another server. That server
/// has the value and this one doesn't, so whatever the fixture answers for
/// such a cell says nothing.
fn fail_foreign_symbols(file_path: &str, batch: &mut Batch, state: &State) -> Failures {
    let mut failures = Vec::new();
    let mut set_here = HashSet::new();
    let items = batch.instructions.iter().zip(&mut batch.expected_result);
    for (instruction, (expected, snooze)) in items {
        match instruction {
            Instruction::CallAndAssign { symbol, .. } => {
                set_here.insert(symbol.as_str());
            }
            Instruction::Call { args, .. } => {
                let foreign = args.iter().find_map(|arg| {
                    let SlimValue::String(text) = arg else {
                        return None;
                    };
                    let name = symbol_name(text).filter(|name| !set_here.contains(name))?;
                    Some((name, state.foreign_server(name)?))
                });
                if let Some((name, set_on)) = foreign {
                    failures.push((
                        foreign_symbol_message(
                            name,
                            set_on,
                            state.server(),
                            file_path,
                            &expected.position,
                        ),
                        snooze.clone(),
                    ));
                    expected.value = ExpectedResultValue::Any;
                }
            }
            _ => {}
        }
    }
    failures
}

fn parse_markdown(file_path: &Path) -> Result<Node> {
    markdown::to_mdast(&read_to_string(file_path)?, &markdown::ParseOptions::gfm())
        .map_err(|err| anyhow!("Error parsing markdown {err}"))
}

/// Prints the verdict of a file and returns whether it fails the run.
pub fn print_fail_or_ok(show_snoozed: bool, failures: Failures) -> bool {
    if !failures.is_empty() {
        println!("{}", failures_label(&failures));
        let mut fail = false;
        for (failure, snooze) in failures {
            let should_snooze = snooze.should_snooze();
            if should_snooze && show_snoozed {
                let snooze_string = format!(" -- snoozed until {snooze}");
                println!(
                    "{failure}{}",
                    if should_snooze { &snooze_string } else { "" }
                );
            } else if !should_snooze {
                println!("{failure}");
            }

            fail |= !should_snooze;
        }
        return fail;
    }
    println!("OK");
    false
}

/// A file is only `SNOOZED` when every failure in it is snoozed. A single
/// real failure makes it a `FAIL`, matching the exit code.
fn failures_label(failures: &[(String, Snooze)]) -> &'static str {
    if failures.iter().all(|(_, snooze)| snooze.should_snooze()) {
        "SNOOZED"
    } else {
        "FAIL"
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn active_snooze() -> Snooze {
        Snooze::snooze(chrono::NaiveDate::from_ymd_opt(9999, 12, 31).unwrap())
    }

    fn expired_snooze() -> Snooze {
        Snooze::snooze(chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap())
    }

    #[test]
    fn label_is_snoozed_only_when_every_failure_is_snoozed() {
        assert_eq!(
            "SNOOZED",
            failures_label(&[("a".into(), active_snooze()), ("b".into(), active_snooze())])
        );
    }

    #[test]
    fn label_is_fail_when_a_real_failure_is_mixed_with_snoozed_ones() {
        assert_eq!(
            "FAIL",
            failures_label(&[
                ("a".into(), active_snooze()),
                ("b".into(), Snooze::not_snooze())
            ])
        );
        assert_eq!(
            "FAIL",
            failures_label(&[
                ("a".into(), active_snooze()),
                ("b".into(), expired_snooze())
            ])
        );
        assert_eq!(
            "FAIL",
            failures_label(&[("a".into(), Snooze::not_snooze())])
        );
    }
    use crate::processor::markdown_commands::{Class, DecisionTableType};
    use crate::servers::Overrides;

    const TWO_SERVERS: &str = "
        [servers.server]
        default = true
        execute_server_command = \"a\"
        [servers.machines]
        execute_server_command = \"b\"
    ";

    fn two_servers() -> Servers {
        Servers::from_config(Some(&TWO_SERVERS.parse().unwrap()), Overrides::default()).unwrap()
    }

    fn server(name: &str, line: usize) -> MarkdownCommand {
        MarkdownCommand::Server {
            name: name.into(),
            position: Position::new(line, 1),
        }
    }

    fn import(line: usize) -> MarkdownCommand {
        MarkdownCommand::Import {
            path: "Some.Path".into(),
            position: Position::new(line, 1),
        }
    }

    #[test]
    fn commands_run_on_the_server_named_above_them() -> Result<()> {
        let servers = two_servers();
        let machines = servers.index_of("machines").unwrap();
        let default = servers.default_index();
        let placed = place_on_servers(
            vec![
                table_at(1, vec![]),
                server("machines", 5),
                import(6),
                table_at(7, vec![]),
                server("server", 11),
                table_at(12, vec![]),
            ],
            &servers,
            "page.md",
        )?;
        assert_eq!(
            vec![
                (default, table_at(1, vec![])),
                (machines, import(6)),
                (machines, table_at(7, vec![])),
                (default, table_at(12, vec![])),
            ],
            placed
        );
        Ok(())
    }

    #[test]
    fn an_unknown_server_is_an_error_at_its_line() {
        let error = place_on_servers(
            vec![table_at(1, vec![]), server("nosuch", 5)],
            &two_servers(),
            "page.md",
        )
        .unwrap_err();
        assert_eq!(
            "Unknown server `nosuch` at page.md:5:1. The configured servers are: `machines`, `server`",
            error.to_string()
        );
    }

    #[test]
    fn a_line_filter_keeps_the_server_directives_above_the_line() -> Result<()> {
        let servers = two_servers();
        let commands = Filter::new().line(8).apply(vec![
            table_at(1, vec![2]),
            server("machines", 5),
            table_at(6, vec![8, 9]),
        ]);
        assert_eq!(
            vec![(servers.index_of("machines").unwrap(), table_at(6, vec![8]))],
            place_on_servers(commands, &servers, "page.md")?
        );
        Ok(())
    }

    #[test]
    fn imports_of_a_server_without_tables_are_dropped() -> Result<()> {
        let mut servers = two_servers();
        let machines = servers.index_of("machines").unwrap();
        let commands = vec![
            import(1),
            server("machines", 2),
            import(3),
            table_at(4, vec![]),
        ];
        let mut placed = place_on_servers(commands.clone(), &servers, "page.md")?;
        drop_idle_imports(&mut placed, &servers);
        assert_eq!(
            vec![(machines, import(3)), (machines, table_at(4, vec![]))],
            placed
        );

        servers.select(&["server".into()])?;
        let mut placed = place_on_servers(commands, &servers, "page.md")?;
        drop_idle_imports(&mut placed, &servers);
        assert_eq!(vec![(machines, table_at(4, vec![]))], placed);
        Ok(())
    }

    #[test]
    fn a_symbol_is_only_known_on_the_server_that_set_it() {
        let mut state = State::new(true);
        state.on_server("server");
        state.set_symbol("ID".into(), InstructionResultValue::String("7".into()));
        assert_eq!(None, state.foreign_server("ID"));
        assert!(state.get_symbol("ID").is_some());
        state.on_server("machines");
        assert_eq!(None, state.get_symbol("ID"));
        assert_eq!(Some("server"), state.foreign_server("ID"));
        assert_eq!(None, state.foreign_server("NEVER"));
        assert_eq!(" on server machines", state.server_suffix());
        state.set_symbol("ID".into(), InstructionResultValue::String("8".into()));
        assert_eq!(None, state.foreign_server("ID"));
        state.on_server("server");
        assert_eq!(
            Some(&InstructionResultValue::String("7".into())),
            state.get_symbol("ID")
        );
    }

    #[test]
    fn test_filter() -> Result<()> {
        let filter = Filter::new().fixture_class("Calculator")?;
        let commands = vec![
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(1, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(10, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::DecisionTable {
                class: Class("Calculator2".into(), Position::new(11, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::DecisionTable {
                class: Class("AnotherFixture".into(), Position::new(11, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::Import {
                path: "Some.Path".into(),
                position: Position::new(0, 1),
            },
        ];
        let filtered = filter.apply(commands);
        assert_eq!(
            vec![
                MarkdownCommand::DecisionTable {
                    class: Class("Calculator".into(), Position::new(1, 1)),
                    r#type: DecisionTableType::MultipleSetterAndGetters,
                    table: vec![],
                    columns: vec![],
                    snoozed: Snooze::not_snooze(),
                },
                MarkdownCommand::DecisionTable {
                    class: Class("Calculator".into(), Position::new(10, 1)),
                    r#type: DecisionTableType::MultipleSetterAndGetters,
                    table: vec![],
                    columns: vec![],
                    snoozed: Snooze::not_snooze(),
                },
                MarkdownCommand::DecisionTable {
                    class: Class("Calculator2".into(), Position::new(11, 1)),
                    r#type: DecisionTableType::MultipleSetterAndGetters,
                    table: vec![],
                    columns: vec![],
                    snoozed: Snooze::not_snooze(),
                },
                MarkdownCommand::Import {
                    path: "Some.Path".into(),
                    position: Position::new(0, 1)
                }
            ],
            filtered
        );
        Ok(())
    }

    #[test]
    fn no_filter() {
        let filter = Filter::new();
        let commands = vec![
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(1, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::Import {
                path: "Some.Path".into(),
                position: Position::new(0, 1),
            },
        ];
        let filtered = filter.apply(commands.clone());
        assert_eq!(filtered, commands);
    }

    fn row(line: usize) -> crate::processor::markdown_commands::TableRow {
        crate::processor::markdown_commands::TableRow {
            position: Position::new(line, 1),
            setters: vec![],
            getters: vec![],
        }
    }

    fn table_at(line: usize, rows: Vec<usize>) -> MarkdownCommand {
        MarkdownCommand::DecisionTable {
            class: Class("Calculator".into(), Position::new(line, 1)),
            r#type: DecisionTableType::MultipleSetterAndGetters,
            table: rows.into_iter().map(row).collect(),
            columns: vec![],
            snoozed: Snooze::not_snooze(),
        }
    }

    #[test]
    fn line_filter_on_a_row_keeps_only_that_row() {
        let filtered = Filter::new()
            .line(12)
            .apply(vec![table_at(1, vec![4, 5]), table_at(10, vec![12, 13])]);
        assert_eq!(vec![table_at(10, vec![12])], filtered);
    }

    #[test]
    fn line_filter_on_a_directive_keeps_every_row() {
        let filtered = Filter::new()
            .line(10)
            .apply(vec![table_at(10, vec![12, 13])]);
        assert_eq!(vec![table_at(10, vec![12, 13])], filtered);
    }

    #[test]
    fn test_filter_line() {
        let filter = Filter::new().line(10);
        let commands = vec![
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(1, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(10, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::Import {
                path: "Some.Path".into(),
                position: Position::new(0, 1),
            },
        ];
        let filtered = filter.apply(commands);
        assert_eq!(
            vec![
                MarkdownCommand::DecisionTable {
                    class: Class("Calculator".into(), Position::new(10, 1)),
                    r#type: DecisionTableType::MultipleSetterAndGetters,
                    table: vec![],
                    columns: vec![],
                    snoozed: Snooze::not_snooze(),
                },
                MarkdownCommand::Import {
                    path: "Some.Path".into(),
                    position: Position::new(0, 1)
                },
            ],
            filtered
        );
    }
}
