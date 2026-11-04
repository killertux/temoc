use crate::app::{get_list_of_files, App};
use crate::jsonl::JsonlOptions;
use crate::processor::Filter;
use crate::slim_server_connector::build_slim_server_connector;
use anyhow::{anyhow, bail, Result};
use clap::{Parser, ValueEnum};
use std::{fs::read_to_string, path::PathBuf};
use toml::Table;

mod app;
mod cancel;
mod captured_server;
mod events;
mod jsonl;
mod port;
mod processor;
mod slim_server_connector;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Text,
    Jsonl,
}

/// Test markdown files using a slim server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Configuration file
    #[arg(short, long)]
    configuration_file: Option<PathBuf>,
    /// Base port to connect to the slim server. Default is 8085
    #[arg(short, long)]
    port: Option<u16>,
    /// The size of the pool of ports to cycle through. Default is 20 (8085 - 8105)
    #[arg(short = 'l', long)]
    pool_size: Option<u16>,
    /// Command to start the slim server
    #[arg(short = 'x', long)]
    execute_server_command: Option<String>,
    /// Recursively traverse files and directories to test
    #[arg(short, long)]
    recursive: bool,
    /// Show snoozed errors
    #[arg(short, long)]
    show_snoozed: bool,
    /// Pipe STDERR and STDOUT of the slim server through the STDOUT
    #[arg(short = 'o', long)]
    pipe_output: bool,
    /// Extension to filter markdown files. Default is md. Not case-sensitive
    #[arg(short, long)]
    extension: Option<String>,
    /// Filter the decision tables by the fixture class. Accept any regex string
    #[arg(short = 'f', long)]
    class_filter: Option<String>,
    /// How to report results. `jsonl` prints one JSON object per line as the tests run. Default is text
    #[arg(long, value_enum)]
    output: Option<OutputFormat>,
    /// List of files to test. Append `:LINE` to select a decision table or a row
    files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if output_format(&args) == OutputFormat::Jsonl {
        std::process::exit(jsonl::run(jsonl_options(args)));
    }
    run_text(args)
}

/// Decides the format before the rest of the configuration is read, because
/// problems with it have to be reported in that format.
fn output_format(args: &Args) -> OutputFormat {
    args.output
        .or_else(|| {
            let configuration_file = args.configuration_file.as_ref()?;
            let config_file = read_to_string(configuration_file).ok()?;
            config_output(&config_file.parse().ok()?).ok()?
        })
        .unwrap_or(OutputFormat::Text)
}

fn jsonl_options(args: Args) -> Result<JsonlOptions> {
    let args = append_config_to_args(args)?;
    let Some(command) = args.execute_server_command else {
        bail!("You need to provide a command to start the slim server")
    };
    let mut filter = Filter::new();
    if let Some(fixture) = args.class_filter {
        filter = filter.fixture_class(&fixture)?;
    }
    Ok(JsonlOptions {
        command,
        port: args.port.unwrap_or(8085),
        pool_size: args.pool_size.unwrap_or(20),
        pipe_output: args.pipe_output,
        recursive: args.recursive,
        extension: args.extension.unwrap_or("md".to_string()).to_lowercase(),
        filter,
        paths: args.files,
    })
}

fn run_text(args: Args) -> Result<()> {
    let args = append_config_to_args(args)?;
    let Some(command) = args.execute_server_command else {
        bail!("You need to provide a command to start the slim server")
    };

    let mut filter = Filter::new();
    if let Some(fixture) = args.class_filter {
        filter = filter.fixture_class(&fixture)?;
    }

    if App::new(
        args.show_snoozed,
        build_slim_server_connector(
            command,
            args.port.unwrap_or(8085),
            args.pool_size.unwrap_or(20),
            args.pipe_output,
        ),
        args.recursive,
        filter,
        args.extension.unwrap_or("md".to_string()).to_lowercase(),
        args.files,
    )
    .run()?
    {
        bail!("Tests executed with error");
    }

    Ok(())
}

fn config_u16(config_file: &Table, key: &str) -> Result<Option<u16>> {
    config_file
        .get(key)
        .map(|value| {
            let value = value
                .as_integer()
                .ok_or_else(|| anyhow!("Expected `{key}` to be a number"))?;
            u16::try_from(value)
                .map_err(|_| anyhow!("Expected `{key}` to be between 0 and 65535 but got {value}"))
        })
        .transpose()
}

fn config_bool(config_file: &Table, key: &str, message: &str) -> Result<bool> {
    config_file.get(key).map_or(Ok(false), |value| {
        value.as_bool().ok_or_else(|| anyhow!("{message}"))
    })
}

fn config_str(config_file: &Table, key: &str, message: &str) -> Result<Option<String>> {
    config_file
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("{message}"))
        })
        .transpose()
}

fn config_output(config_file: &Table) -> Result<Option<OutputFormat>> {
    config_str(config_file, "output", "Expected `output` to be a string")?
        .map(|output| {
            OutputFormat::from_str(&output, false).map_err(|_| {
                anyhow!("Expected `output` to be `text` or `jsonl` but got `{output}`")
            })
        })
        .transpose()
}

fn append_config_to_args(mut args: Args) -> Result<Args> {
    match &args.configuration_file {
        None => Ok(args),
        Some(configuration_file) => {
            let config_file = read_to_string(configuration_file)?.parse::<Table>()?;
            args.port = args.port.or(config_u16(&config_file, "port")?);
            args.pool_size = args.pool_size.or(config_u16(&config_file, "pool_size")?);
            args.output = args.output.or(config_output(&config_file)?);
            args.recursive = args.recursive
                || config_bool(
                    &config_file,
                    "recursive",
                    "Expect the recursive to be a boolean",
                )?;
            args.show_snoozed = args.show_snoozed
                || config_bool(
                    &config_file,
                    "show_snoozed",
                    "Expect the show_snoozed to be a boolean",
                )?;
            args.pipe_output = args.pipe_output
                || config_bool(
                    &config_file,
                    "pipe_output",
                    "Expect the verbose to be a boolean",
                )?;
            if args.execute_server_command.is_none() {
                args.execute_server_command = config_str(
                    &config_file,
                    "execute_server_command",
                    "Expect the slim server command to be a string",
                )?;
            }
            if args.extension.is_none() {
                args.extension = config_str(
                    &config_file,
                    "extension",
                    "Expect the slim server command to be a string",
                )?;
            }
            if args.files.is_empty() {
                let test_dir = config_str(
                    &config_file,
                    "test_dir",
                    "Expect the test dir to be a string",
                )?
                .ok_or(anyhow!("You need to specify a test file or a test dir"))?;
                args.files = get_list_of_files(test_dir)?;
            }
            Ok(args)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn config(toml: &str) -> Table {
        toml.parse().unwrap()
    }

    #[test]
    fn config_u16_reads_valid_values() {
        let config = config("port = 8085");
        assert_eq!(Some(8085), config_u16(&config, "port").unwrap());
        assert_eq!(None, config_u16(&config, "pool_size").unwrap());
    }

    #[test]
    fn config_u16_rejects_out_of_range_values() {
        for value in ["70000", "65537", "-1"] {
            let config = config(&format!("port = {value}"));
            let error = config_u16(&config, "port").unwrap_err();
            assert_eq!(
                format!("Expected `port` to be between 0 and 65535 but got {value}"),
                error.to_string()
            );
        }
    }

    #[test]
    fn config_u16_rejects_values_that_are_not_numbers() {
        let error = config_u16(&config("pool_size = \"ten\""), "pool_size").unwrap_err();
        assert_eq!("Expected `pool_size` to be a number", error.to_string());
    }
}
