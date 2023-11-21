use self::slim_deserialize::FromSlimReader;
use self::slim_serialize::ToSlimString;
use anyhow::{anyhow, bail, Result};
use std::{
    fmt::Display,
    io::{BufReader, Read, Write},
};
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

impl<R, W> SlimConnection<R, W>
where
    R: Read,
    W: Write,
{
    pub fn new(mut reader: R, writer: W) -> Result<Self> {
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

    pub fn send_instructions(&mut self, data: &[Instruction]) -> Result<Vec<InstructionResult>> {
        self.writer.write_all(data.to_slim_string().as_bytes())?;
        Vec::from_reader(&mut self.reader)
    }

    pub fn close(mut self) -> Result<()> {
        self.say_goodbye()?;
        self.closed = true;
        Ok(())
    }

    fn say_goodbye(&mut self) -> Result<()> {
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
impl SlimVersion {
    fn from_str(string: impl AsRef<str>) -> Result<Self> {
        let (_, version) = string
            .as_ref()
            .split_once(" -- ")
            .ok_or(anyhow!("Invalid slim version string"))?;
        Ok(match version.trim() {
            "V0.3" => SlimVersion::V0_3,
            "V0.4" => SlimVersion::V0_4,
            "V0.5" => SlimVersion::V0_5,
            v => bail!("Version {v} not recognized"),
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Id(Ulid);

impl Id {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn from_string(string: String) -> Result<Self> {
        Ok(Self(Ulid::from_string(&string)?))
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
    Ok {
        id: Id,
    },
    Null {
        id: Id,
    },
    Exception {
        id: Id,
        message: String,
        _complete_message: String,
    },
    String {
        id: Id,
        value: String,
    },
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use super::*;
    use anyhow::Result;

    #[test]
    fn test_simple_connection() -> Result<()> {
        let mut writer = Vec::new();
        let connection =
            SlimConnection::new(Cursor::new(b"Slim -- V0.5\n"), Cursor::new(&mut writer))?;
        assert_eq!(SlimVersion::V0_5, connection._version);
        drop(connection);
        assert_eq!("000003:bye".to_string(), String::from_utf8_lossy(&writer));
        Ok(())
    }

    #[test]
    fn test_send_instructions_connection() -> Result<()> {
        let mut writer = Vec::new();
        let mut connection = SlimConnection::new(
            Cursor::new(b"Slim -- V0.5\n000197:[000003:000053:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000002:OK:]:000055:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:null:]:000056:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000005:Hello:]:]"),
            Cursor::new(&mut writer),
        )?;
        let id = Id::from_string("01HFM0NQM3ZS6BBX0ZH6VA6DJX".into()).unwrap();
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
                InstructionResult::Null { id: id.clone() },
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
