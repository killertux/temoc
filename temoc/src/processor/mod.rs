use self::{
    markdown_commands::{get_commands_from_markdown, Snooze},
    slim_instructions_from_commands::ExpectedResulWithSnooze,
};
use crate::processor::{
    slim_instructions_from_commands::get_instructions_from_commands,
    validate_result::validate_result,
};
use anyhow::{anyhow, Result};
use markdown::mdast::Node;
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
) -> Result<(Vec<Instruction>, Vec<ExpectedResulWithSnooze>)> {
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
