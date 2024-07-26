use self::{
    markdown_commands::{get_commands_from_markdown, Snooze},
    slim_instructions_from_commands::ExpectedResulWithSnooze,
};
use crate::processor::markdown_commands::MarkdownCommand;
use crate::processor::{
    slim_instructions_from_commands::get_instructions_from_commands,
    validate_result::validate_result,
};
use anyhow::{anyhow, Result};
use markdown::mdast::Node;
use regex::Regex;
use slim_protocol::{Instruction, SlimConnection};
use std::{
    fs::read_to_string,
    io::{Read, Write},
    path::Path,
};

mod markdown_commands;
mod slim_instructions_from_commands;
mod validate_result;

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
        commands
            .into_iter()
            .filter(|command| {
                self.filters.iter().all(|filter| match filter {
                    FilterType::FixtureClass(regex) => match command {
                        MarkdownCommand::DecisionTable { class, .. } => regex.is_match(&class.0),
                        MarkdownCommand::Import { .. } => true,
                    },
                    FilterType::Line(line) => match command {
                        MarkdownCommand::DecisionTable { class, .. } => class.1.line() == *line,
                        MarkdownCommand::Import { .. } => true,
                    },
                })
            })
            .collect()
    }
}

pub fn process_markdown_into_instructions(
    file_path: impl AsRef<Path>,
    filter: &Filter,
) -> Result<(Vec<Instruction>, Vec<ExpectedResulWithSnooze>)> {
    let file_path = file_path.as_ref();
    let file_path_display = file_path.display().to_string();
    print!("Testing file {}...", file_path_display);
    let markdown = parse_markdown(file_path)?;
    let commands = filter.apply(get_commands_from_markdown(markdown, file_path_display)?);
    if !commands
        .iter()
        .any(|command| matches!(command, MarkdownCommand::DecisionTable { .. }))
    {
        return Ok((vec![], vec![]));
    }
    get_instructions_from_commands(commands)
}

pub fn execute_instructions_and_print_result<R: Read, W: Write>(
    connection: &mut SlimConnection<R, W>,
    file_path: &str,
    instructions: Vec<Instruction>,
    expected_result: Vec<ExpectedResulWithSnooze>,
    show_snoozed: bool,
) -> Result<bool> {
    let result = connection.send_instructions(&instructions)?;
    let failures = validate_result(file_path, expected_result, result)?;
    print_fail_or_ok(show_snoozed, failures)
}

fn parse_markdown(file_path: &Path) -> Result<Node> {
    markdown::to_mdast(&read_to_string(file_path)?, &markdown::ParseOptions::gfm())
        .map_err(|err| anyhow!("Error parsing markdown {err}"))
}

fn print_fail_or_ok(show_snoozed: bool, failures: Vec<(String, Snooze)>) -> Result<bool> {
    if !failures.is_empty() {
        if failures.iter().any(|(_, snoose)| snoose.should_snooze()) {
            println!("SNOOZED");
        } else {
            println!("FAIL");
        }
        let mut fail = false;
        for (failure, snooze) in failures.into_iter() {
            let should_snooze = snooze.should_snooze();
            if should_snooze && show_snoozed {
                let snooze_string = format!(" -- snoozed until {}", &snooze);
                println!(
                    "{failure}{}",
                    if should_snooze { &snooze_string } else { "" }
                );
            } else if !should_snooze {
                println!("{failure}");
            }

            fail |= !should_snooze;
        }
        return Ok(fail);
    }
    println!("OK");
    Ok(false)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::processor::markdown_commands::{Class, DecisionTableType, Position};

    #[test]
    fn test_filter() -> Result<()> {
        let filter = Filter::new().fixture_class("Calculator")?;
        let commands = vec![
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(1, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(10, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::DecisionTable {
                class: Class("Calculator2".into(), Position::new(11, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::DecisionTable {
                class: Class("AnotherFixture".into(), Position::new(11, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
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
                    snoozed: Snooze::not_snooze(),
                },
                MarkdownCommand::DecisionTable {
                    class: Class("Calculator".into(), Position::new(10, 1)),
                    r#type: DecisionTableType::MultipleSetterAndGetters,
                    table: vec![],
                    snoozed: Snooze::not_snooze(),
                },
                MarkdownCommand::DecisionTable {
                    class: Class("Calculator2".into(), Position::new(11, 1)),
                    r#type: DecisionTableType::MultipleSetterAndGetters,
                    table: vec![],
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

    #[test]
    fn test_filter_line() -> Result<()> {
        let filter = Filter::new().line(10);
        let commands = vec![
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(1, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
                snoozed: Snooze::not_snooze(),
            },
            MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(10, 1)),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![],
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
                    snoozed: Snooze::not_snooze(),
                },
                MarkdownCommand::Import {
                    path: "Some.Path".into(),
                    position: Position::new(0, 1)
                },
            ],
            filtered
        );
        Ok(())
    }
}
