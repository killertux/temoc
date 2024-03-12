pub use self::slim_deserialize::{FromSlimReader, FromSlimReaderError};
pub use self::slim_serialize::ToSlimString;
use std::{
    fmt::Display,
    io::{BufReader, Read, Write},
};
use thiserror::Error;
use ulid::Ulid;

mod slim_deserialize;
mod slim_serialize;

pub struct SlimConnection<R, W>
where
    R: Read,
    W: Write,
{
    reader: BufReader<R>,
    writer: W,
    _version: SlimVersion,
    closed: bool,
}

#[derive(Debug, Error)]
pub enum NewSlimConnectionError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SlimVersionReadError(#[from] SlimVersionReadError),
}

#[derive(Debug, Error)]
pub enum SendInstructionsError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    FromSlimReaderError(#[from] FromSlimReaderError),
}

impl<R, W> SlimConnection<R, W>
where
    R: Read,
    W: Write,
{
    pub fn new(mut reader: R, writer: W) -> Result<Self, NewSlimConnectionError> {
        let mut buf = [0_u8; 13];
        reader.read_exact(&mut buf)?;
        let version = SlimVersion::from_str(String::from_utf8_lossy(&buf))?;
        Ok(Self {
            reader: BufReader::new(reader),
            writer,
            _version: version,
            closed: false,
        })
    }

    pub fn send_instructions(
        &mut self,
        data: &[Instruction],
    ) -> Result<Vec<InstructionResult>, SendInstructionsError> {
        self.writer.write_all(data.to_slim_string().as_bytes())?;
        Ok(Vec::from_reader(&mut self.reader)?)
    }

    pub fn close(mut self) -> Result<(), std::io::Error> {
        self.say_goodbye()?;
        self.closed = true;
        Ok(())
    }

    fn say_goodbye(&mut self) -> Result<(), std::io::Error> {
        self.writer.write_all("bye".to_slim_string().as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }
}

impl<R, W> Drop for SlimConnection<R, W>
where
    R: Read,
    W: Write,
{
    fn drop(&mut self) {
        if !self.closed {
            self.say_goodbye().expect("Error sending goodbye");
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SlimVersion {
    V0_3,
    V0_4,
    V0_5,
}

#[derive(Debug, Error)]
pub enum SlimVersionReadError {
    #[error("Invalid slim version string")]
    Invalid,
    #[error("Version {0} not recognized")]
    NotRecognized(String),
}

impl SlimVersion {
    fn from_str(string: impl AsRef<str>) -> Result<Self, SlimVersionReadError> {
        let (_, version) = string
            .as_ref()
            .split_once(" -- ")
            .ok_or(SlimVersionReadError::Invalid)?;
        Ok(match version.trim() {
            "V0.3" => SlimVersion::V0_3,
            "V0.4" => SlimVersion::V0_4,
            "V0.5" => SlimVersion::V0_5,
            v => return Err(SlimVersionReadError::NotRecognized(v.into())),
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Id(String);

impl Id {
    pub fn new() -> Self {
        Self::default()
    }
}

impl From<String> for Id {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Id {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl Default for Id {
    fn default() -> Self {
        Self(Ulid::new().to_string())
    }
}

impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Instruction {
    Import {
        id: Id,
        path: String,
    },
    Make {
        id: Id,
        instance: String,
        class: String,
        args: Vec<String>,
    },
    Call {
        id: Id,
        instance: String,
        function: String,
        args: Vec<String>,
    },
    #[allow(dead_code)]
    CallAndAssign {
        id: Id,
        symbol: String,
        instance: String,
        function: String,
        args: Vec<String>,
    },
    #[allow(dead_code)]
    Assign {
        id: Id,
        symbol: String,
        value: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstructionResult {
    Ok { id: Id },
    Void { id: Id },
    Exception { id: Id, message: ExceptionMessage },
    String { id: Id, value: String },
}

impl Display for InstructionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstructionResult::Ok { id: _ } => write!(f, "OK")?,
            InstructionResult::Void { id: _ } => write!(f, "VOID")?,
            InstructionResult::Exception { id: _, message } => write!(
                f,
                "Exception `{}`",
                message.pretty_message().unwrap_or(message.raw_message())
            )?,
            InstructionResult::String { id: _, value } => write!(f, "`{}`", value)?,
        }
        Ok(())
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct ExceptionMessage(String);

#[derive(Debug, Error)]
#[error("Failed processing exception {0}")]
pub struct ExceptionPrettyMessageError(String);

impl ExceptionMessage {
    pub fn new(message: String) -> Self {
        Self(message)
    }

    pub fn raw_message(&self) -> &str {
        &self.0
    }

    pub fn pretty_message(&self) -> Result<&str, ExceptionPrettyMessageError> {
        if let Some(pos) = self.0.find("message:<<") {
            let (_, rest) = self.0.split_at(pos + 10);
            let Some((message, _)) = rest.split_once(">>") else {
                return Err(ExceptionPrettyMessageError(self.0.clone()));
            };
            Ok(message)
        } else {
            Ok(&self.0)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ByeOrSlimInstructions {
    Bye,
    Instructions(Vec<Instruction>),
}

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::io::Cursor;

    use super::*;

    #[test]
    fn test_simple_connection() -> Result<(), Box<dyn Error>> {
        let mut writer = Vec::new();
        let connection =
            SlimConnection::new(Cursor::new(b"Slim -- V0.5\n"), Cursor::new(&mut writer))?;
        assert_eq!(SlimVersion::V0_5, connection._version);
        drop(connection);
        assert_eq!("000003:bye".to_string(), String::from_utf8_lossy(&writer));
        Ok(())
    }

    #[test]
    fn test_send_instructions_connection() -> Result<(), Box<dyn Error>> {
        let mut writer = Vec::new();
        let mut connection = SlimConnection::new(
            Cursor::new(b"Slim -- V0.5\n000197:[000003:000053:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000002:OK:]:000055:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:null:]:000056:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000005:Hello:]:]"),
            Cursor::new(&mut writer),
        )?;
        let id = Id::from("01HFM0NQM3ZS6BBX0ZH6VA6DJX");
        let result = connection.send_instructions(&[
            Instruction::Import {
                id: id.clone(),
                path: "Path".into(),
            },
            Instruction::Call {
                id: id.clone(),
                instance: "Instance".into(),
                function: "Function".into(),
                args: Vec::new(),
            },
            Instruction::Call {
                id: id.clone(),
                instance: "Instance".into(),
                function: "Function".into(),
                args: Vec::new(),
            },
        ])?;
        drop(connection);
        assert_eq!(
            vec![
                InstructionResult::Ok { id: id.clone() },
                InstructionResult::String {
                    id: id.clone(),
                    value: "null".into()
                },
                InstructionResult::String {
                    id: id.clone(),
                    value: "Hello".into()
                }
            ],
            result
        );
        assert_eq!(
            "000276:[000003:000069:[000003:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000006:import:000004:Path:]:000087:[000004:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:call:000008:Instance:000008:Function:]:000087:[000004:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:call:000008:Instance:000008:Function:]:]000003:bye".to_string(),
            String::from_utf8_lossy(&writer)
        );
        Ok(())
    }
}
