pub use self::slim_instructions_from_commands::{file_hooks, Batch};
use self::{
    markdown_commands::{get_commands_from_markdown, Snooze},
    slim_instructions_from_commands::{get_file_plan, ExpectedResulWithSnooze, FilePlan},
};
use crate::processor::markdown_commands::MarkdownCommand;
use crate::processor::{
    slim_instructions_from_commands::get_instructions_from_commands,
    validate_result::validate_result,
};
use anyhow::{anyhow, Result};
use markdown::mdast::Node;
use regex::Regex;
use slim_protocol::{Instruction, InstructionResultValue, SlimConnection};
use std::collections::HashMap;
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

#[derive(Debug, Default)]
pub struct State {
    symbols: HashMap<String, InstructionResultValue>,
}

impl State {
    pub fn set_symbol(&mut self, key: String, value: InstructionResultValue) {
        self.symbols.insert(key, value);
    }

    pub fn get_symbol(&self, key: &str) -> Option<&InstructionResultValue> {
        self.symbols.get(key)
    }
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
                        MarkdownCommand::Import { .. } => true,
                    },
                    FilterType::Line(line) => match command {
                        MarkdownCommand::DecisionTable { class, table, .. } => {
                            class.1.line() == *line
                                || table.iter().any(|row| row.position.line() == *line)
                        }
                        MarkdownCommand::Import { .. } => true,
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

pub fn process_markdown_into_instructions(
    file_path: impl AsRef<Path>,
    filter: &Filter,
) -> Result<(Vec<Instruction>, Vec<ExpectedResulWithSnooze>)> {
    let file_path = file_path.as_ref();
    print!("Testing file {}...", file_path.display());
    let commands = load_commands(file_path, filter)?;
    if !commands
        .iter()
        .any(|command| matches!(command, MarkdownCommand::DecisionTable { .. }))
    {
        return Ok((vec![], vec![]));
    }
    Ok(get_instructions_from_commands(commands))
}

fn load_commands(file_path: &Path, filter: &Filter) -> Result<Vec<MarkdownCommand>> {
    let markdown = parse_markdown(file_path)?;
    Ok(filter.apply(get_commands_from_markdown(
        markdown,
        file_path.display().to_string(),
    )?))
}

/// Reads a file into the batches that run it. `None` when no table matches.
pub fn load_file_plan(file_path: &Path, filter: &Filter) -> Result<Option<FilePlan>> {
    let commands = load_commands(file_path, filter)?;
    if !commands
        .iter()
        .any(|command| matches!(command, MarkdownCommand::DecisionTable { .. }))
    {
        return Ok(None);
    }
    Ok(Some(get_file_plan(commands)))
}

/// What went wrong, and the snooze it is under.
pub type Failures = Vec<(String, Snooze)>;

/// Sends one batch and returns what went wrong in it.
pub fn execute_instructions<R: Read, W: Write>(
    connection: &mut SlimConnection<R, W>,
    file_path: &str,
    batch: Batch,
    state: &mut State,
) -> Result<Failures> {
    let result = connection.send_instructions(&batch.instructions)?;
    validate_result(file_path, batch.expected_result, result, state)
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
    use crate::processor::markdown_commands::{Class, DecisionTableType, Position};

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
