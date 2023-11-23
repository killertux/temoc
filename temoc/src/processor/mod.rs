use self::markdown_commands::{get_commands_from_markdown, Snooze};
use crate::processor::{
    slim_instructions_from_commands::get_instructions_from_commands,
    validate_result::validate_result,
};
use anyhow::{anyhow, Result};
use markdown::mdast::Node;
use slim_protocol::SlimConnection;
use std::{
    fs::read_to_string,
    io::{Read, Write},
    path::Path,
};

mod markdown_commands;
mod slim_instructions_from_commands;
mod validate_result;

pub fn process_markdown<R: Read, W: Write>(
    connection: &mut SlimConnection<R, W>,
    show_snoozed: bool,
    file_path: impl AsRef<Path>,
) -> Result<bool> {
    let file_path = file_path.as_ref();
    let file_path_display = file_path.display();
    print!("Testing file {}...", file_path_display);
    let markdown = parse_markdown(file_path)?;
    let commands = get_commands_from_markdown(markdown)?;
    let (instructions, expected_result) = get_instructions_from_commands(commands)?;
    let result = connection.send_instructions(&instructions)?;
    let failures = validate_result(file_path_display, expected_result, result)?;
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
