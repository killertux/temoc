use self::{
    markdown_commands::{get_commands_from_markdown, Snooze},
    slim_instructions_from_commands::{ExpectedResulWithSnooze, LoggerEvent},
};
use crate::{
    logger::TestLogger,
    processor::{
        slim_instructions_from_commands::get_instructions_from_commands,
        validate_result::validate_result,
    },
};
use anyhow::{anyhow, Result};
use markdown::mdast::Node;
pub use markdown_commands::Position;
use slim_protocol::{Instruction, SlimConnection};
use std::{
    fs::read_to_string,
    io::{Read, Write},
    path::Path,
};

mod markdown_commands;
mod slim_instructions_from_commands;
mod validate_result;

pub fn process_markdown_into_instructions(
    file_path: impl AsRef<Path>,
) -> Result<(
    Vec<(LoggerEvent, Vec<Instruction>)>,
    Vec<Vec<ExpectedResulWithSnooze>>,
)> {
    let file_path = file_path.as_ref();
    let file_path_display = file_path.display();
    print!("Testing file {}...", file_path_display);
    let markdown = parse_markdown(file_path)?;
    let commands = get_commands_from_markdown(markdown)?;
    get_instructions_from_commands(commands)
}

pub fn execute_instructions_and_print_result<R: Read, W: Write>(
    connection: &mut SlimConnection<R, W>,
    file_path: &str,
    instructions: Vec<(LoggerEvent, Vec<Instruction>)>,
    expected_result: Vec<Vec<ExpectedResulWithSnooze>>,
    show_snoozed: bool,
    logger: &mut Box<dyn TestLogger>,
) -> Result<bool> {
    let mut failures = Vec::new();
    for ((logger_event, instructions), expected_result) in
        instructions.into_iter().zip(expected_result.into_iter())
    {
        match logger_event {
            LoggerEvent::Nop => {
                let result = connection.send_instructions(&instructions)?;
                failures.append(&mut validate_result(file_path, expected_result, result)?);
            }
            LoggerEvent::RowExecution {
                decision_table_name,
                row_number,
                position,
            } => {
                logger.row_started(&decision_table_name, row_number, position)?;
                let result = connection.send_instructions(&instructions)?;
                let mut result = &mut validate_result(file_path, expected_result, result)?;
                for (failure, snooze) in result.iter() {
                    if snooze.should_snooze() {
                        logger.row_snoozed(failure)?;
                    } else {
                        logger.row_failed(failure)?;
                    }
                }
                failures.append(&mut result);
                logger.row_finished()?;
            }
        }
    }
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
