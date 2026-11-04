use crate::processor::{
    execute_instructions_and_print_result, process_markdown_into_instructions, Filter, State,
};
use crate::slim_server_connector::SlimServerConnector;
use anyhow::Result;
use slim_protocol::SlimConnection;
use std::fs::{metadata, read_dir};
use std::num::ParseIntError;
use std::path::{Path, PathBuf};

pub struct App {
    show_snoozed: bool,
    slim_server_connector: Box<dyn SlimServerConnector>,
    recursive: bool,
    extension: String,
    filter: Filter,
    paths: Vec<PathBuf>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        show_snoozed: bool,
        slim_server_connector: Box<dyn SlimServerConnector>,
        recursive: bool,
        filter: Filter,
        extension: String,
        paths: Vec<PathBuf>,
    ) -> Self {
        App {
            show_snoozed,
            slim_server_connector,
            recursive,
            extension,
            filter,
            paths,
        }
    }

    pub fn run(mut self) -> Result<bool> {
        self.process_paths(self.paths.clone())
    }

    pub fn process_paths(&mut self, paths: Vec<PathBuf>) -> Result<bool> {
        let mut fail = false;
        for path in paths {
            fail |= self.process_path(path)?;
        }
        Ok(fail)
    }

    pub fn process_path(&mut self, path: impl AsRef<Path>) -> Result<bool> {
        let (path, line) = split_selector(path.as_ref());
        let mut filter = self.filter.clone();
        if let Some(line) = line {
            filter = filter.line(line);
        }
        let metadata = metadata(&path)?;
        if metadata.is_dir() && self.recursive {
            return self.process_paths(get_list_of_files(&path)?);
        }
        if metadata.is_file() && has_extension(&path, &self.extension) {
            return self.process_file(&path, &filter);
        }
        Ok(false)
    }

    fn process_file(&mut self, file: impl AsRef<Path>, filter: &Filter) -> Result<bool> {
        let (instructions, expected_result) = process_markdown_into_instructions(&file, filter)?;
        if instructions.is_empty() {
            println!("NONE");
            return Ok(false);
        }
        let mut slim_server = self.slim_server_connector.start_and_connect()?;
        let mut state = State::default();
        let mut connection = SlimConnection::new(slim_server.reader()?, slim_server.writer()?)?;
        let fail = execute_instructions_and_print_result(
            &mut connection,
            &file.as_ref().to_string_lossy(),
            &instructions,
            expected_result,
            self.show_snoozed,
            &mut state,
        )?;
        connection.close()?;
        slim_server.close()?;
        Ok(fail)
    }
}

/// Splits `file.md:12` into the path and the selected line.
pub fn split_selector(path: &Path) -> (PathBuf, Option<usize>) {
    if let Ok(line) = get_line_from_path(path) {
        if let Some(remaining) = path.to_string_lossy().strip_suffix(&format!(":{line}")) {
            return (PathBuf::from(remaining), Some(line));
        }
    }
    (path.to_path_buf(), None)
}

fn get_line_from_path(path: &Path) -> Result<usize, ParseIntError> {
    let line = path
        .display()
        .to_string()
        .chars()
        .rev()
        .take_while(|c| c.is_numeric())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    line.parse::<usize>()
}

pub fn has_extension(path: &Path, extension: &str) -> bool {
    path.to_string_lossy().to_lowercase().ends_with(extension)
}

pub fn get_list_of_files(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    Ok(read_dir(dir.as_ref())?
        .map(|file| file.map(|file| file.path().clone()))
        .collect::<Result<Vec<PathBuf>, std::io::Error>>()?)
}
