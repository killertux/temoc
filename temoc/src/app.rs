use crate::processor::{
    execute_instructions_and_print_result, process_markdown_into_instructions, Filter,
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
        let mut path = path.as_ref().to_path_buf();
        let mut filter = self.filter.clone();
        if let Ok(line) = Self::get_line_from_path(&path) {
            if let Some(remaining) = path.to_string_lossy().strip_suffix(&format!(":{line}")) {
                path = PathBuf::from(remaining);
                filter = filter.line(line);
            }
        }
        let metadata = metadata(&path)?;
        if metadata.is_dir() && self.recursive {
            return self.process_paths(get_list_of_files(&path)?);
        }
        if metadata.is_file() && self.is_correct_extension(&path) {
            return self.process_file(&path, filter);
        }
        Ok(false)
    }

    fn get_line_from_path(path: &impl AsRef<Path>) -> Result<usize, ParseIntError> {
        let line = path
            .as_ref()
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

    fn is_correct_extension(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref()
            .to_string_lossy()
            .to_lowercase()
            .ends_with(&self.extension)
    }

    fn process_file(&mut self, file: impl AsRef<Path>, filter: Filter) -> Result<bool> {
        let (instructions, expected_result) = process_markdown_into_instructions(&file, &filter)?;
        if instructions.is_empty() {
            println!("NONE");
            return Ok(false);
        }
        let mut slim_server = self.slim_server_connector.start_and_connect()?;
        let mut connection = SlimConnection::new(slim_server.reader()?, slim_server.writer()?)?;
        let fail = execute_instructions_and_print_result(
            &mut connection,
            &file.as_ref().to_string_lossy(),
            instructions,
            expected_result,
            self.show_snoozed,
        )?;
        connection.close()?;
        slim_server.close()?;
        Ok(fail)
    }
}

pub fn get_list_of_files(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    Ok(read_dir(dir.as_ref())?
        .map(|file| file.map(|file| file.path().to_path_buf()))
        .collect::<Result<Vec<PathBuf>, std::io::Error>>()?)
}
