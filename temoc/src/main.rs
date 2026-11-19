use crate::app::{get_list_of_files, App, Options};
use crate::config::{config_bool, config_str};
use crate::jsonl::JsonlOptions;
use crate::processor::Filter;
use crate::servers::{Overrides, Servers};
use anyhow::{anyhow, bail, Result};
use clap::{Parser, ValueEnum};
use std::{fs::read_to_string, path::PathBuf};
use toml::Table;

mod app;
mod cancel;
mod captured_server;
mod config;
mod events;
mod jsonl;
mod port;
mod processor;
mod server_pool;
mod servers;
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
    /// Start the slim server once and use it for every file, instead of starting one for each file
    #[arg(long)]
    persistent_server: bool,
    /// Fixture class whose `beginFile` and `endFile` methods are called around every file
    #[arg(long, value_name = "CLASS")]
    file_hooks: Option<String>,
    /// Run only the tables of this server. Repeat it, or separate names with commas, to select several
    #[arg(long = "server", value_name = "NAME", value_delimiter = ',')]
    servers: Vec<String>,
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

/// The command line and the configuration file put together.
struct Settings {
    servers: Servers,
    recursive: bool,
    show_snoozed: bool,
    extension: String,
    filter: Filter,
    files: Vec<PathBuf>,
}

fn jsonl_options(args: Args) -> Result<JsonlOptions> {
    let settings = settings(args)?;
    Ok(JsonlOptions {
        servers: settings.servers,
        recursive: settings.recursive,
        extension: settings.extension,
        filter: settings.filter,
        paths: settings.files,
    })
}

fn run_text(args: Args) -> Result<()> {
    let settings = settings(args)?;
    if App::new(
        Options {
            show_snoozed: settings.show_snoozed,
            recursive: settings.recursive,
            extension: settings.extension,
            filter: settings.filter,
            paths: settings.files,
        },
        settings.servers,
    )
    .run()?
    {
        bail!("Tests executed with error");
    }

    Ok(())
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

fn settings(mut args: Args) -> Result<Settings> {
    let config_file = match &args.configuration_file {
        Some(configuration_file) => Some(read_to_string(configuration_file)?.parse::<Table>()?),
        None => None,
    };
    if let Some(config_file) = &config_file {
        config_output(config_file)?;
        args.recursive = args.recursive
            || config_bool(
                config_file,
                "recursive",
                "Expect the recursive to be a boolean",
            )?;
        args.show_snoozed = args.show_snoozed
            || config_bool(
                config_file,
                "show_snoozed",
                "Expect the show_snoozed to be a boolean",
            )?;
        if args.extension.is_none() {
            args.extension = config_str(
                config_file,
                "extension",
                "Expect the extension to be a string",
            )?;
        }
    }
    let mut servers = Servers::from_config(
        config_file.as_ref(),
        Overrides {
            command: args.execute_server_command,
            port: args.port,
            pool_size: args.pool_size,
            persistent: args.persistent_server,
            file_hooks: args.file_hooks,
            pipe_output: args.pipe_output,
        },
    )?;
    servers.select(&args.servers)?;
    if let (Some(config_file), true) = (&config_file, args.files.is_empty()) {
        let test_dir = config_str(
            config_file,
            "test_dir",
            "Expect the test dir to be a string",
        )?
        .ok_or(anyhow!("You need to specify a test file or a test dir"))?;
        args.files = get_list_of_files(test_dir)?;
    }
    let mut filter = Filter::new();
    if let Some(fixture) = args.class_filter {
        filter = filter.fixture_class(&fixture)?;
    }
    Ok(Settings {
        servers,
        recursive: args.recursive,
        show_snoozed: args.show_snoozed,
        extension: args.extension.unwrap_or("md".to_string()).to_lowercase(),
        filter,
        files: args.files,
    })
}
