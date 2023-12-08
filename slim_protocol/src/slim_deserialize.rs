use std::io::{self, BufRead};
use thiserror::Error;

use crate::{ExceptionMessage, Instruction};

use super::{ByeOrSlimInstructions, Id, InstructionResult};

#[derive(Debug, Error)]
pub enum FromSlimReaderError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub trait FromSlimReader {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError>
    where
        Self: Sized;
}

impl FromSlimReader for String {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        let len = read_len(reader)?;
        let mut buffer = vec![0_u8; len];
        reader.read_exact(&mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }
}

impl<T, const S: usize> FromSlimReader for [T; S]
where
    T: FromSlimReader,
{
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        let result = Vec::from_reader(reader)?;
        result
            .try_into()
            .map_err(|_| FromSlimReaderError::Other("Missing elements from array".into()))
    }
}

impl<T> FromSlimReader for Vec<T>
where
    T: FromSlimReader,
{
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        let _ = read_len(reader)?; // TODO: Validate this len against the read bytes
        let mut result = Vec::new();
        reader.read_expected_byte(b'[')?;
        let n_elements = read_len(reader)?;
        for _ in 0..n_elements {
            result.push(T::from_reader(reader)?);
            reader.read_expected_byte(b':')?;
        }
        reader.read_expected_byte(b']')?;
        Ok(result)
    }
}

impl FromSlimReader for InstructionResult {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        let [id, value] = <[String; 2]>::from_reader(reader)?;
        let id = Id::from(id);
        Ok(match value.as_str() {
            "OK" => InstructionResult::Ok { id },
            "/__VOID__/" => InstructionResult::Void { id },
            other => {
                if let Some(message) = other.strip_prefix("__EXCEPTION__:") {
                    InstructionResult::Exception {
                        id,
                        message: ExceptionMessage::new(message.into()),
                    }
                } else {
                    InstructionResult::String { id, value }
                }
            }
        })
    }
}

impl FromSlimReader for Instruction {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError>
    where
        Self: Sized,
    {
        let mut data: Vec<String> = Vec::from_reader(reader)?;
        data.reverse();

        let id = Id::from(
            data.pop()
                .ok_or(FromSlimReaderError::Other("Expected data".into()))?,
        );
        match data
            .pop()
            .ok_or(FromSlimReaderError::Other("Expectd instruction".into()))?
            .as_str()
        {
            "import" => {
                let path = data
                    .pop()
                    .ok_or(FromSlimReaderError::Other("Expected path".into()))?;
                Ok(Instruction::Import { id, path })
            }
            "make" => {
                let instance = data
                    .pop()
                    .ok_or(FromSlimReaderError::Other("Expected instance".into()))?;
                let class = data
                    .pop()
                    .ok_or(FromSlimReaderError::Other("Expected class".into()))?;
                data.reverse();
                Ok(Instruction::Make {
                    id,
                    instance,
                    class,
                    args: data,
                })
            }
            "call" => {
                let instance = data
                    .pop()
                    .ok_or(FromSlimReaderError::Other("Expected instance".into()))?;
                let function = data
                    .pop()
                    .ok_or(FromSlimReaderError::Other("Expected function".into()))?;
                data.reverse();
                Ok(Instruction::Call {
                    id,
                    instance,
                    function,
                    args: data,
                })
            }
            other => todo!("Not implemented {other}"),
        }
    }
}

impl FromSlimReader for ByeOrSlimInstructions {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError>
    where
        Self: Sized,
    {
        let _ = read_len(reader)?;
        match reader.read_byte()? {
            b'[' => {
                let mut result = Vec::new();
                let n_elements = read_len(reader)?;
                for _ in 0..n_elements {
                    result.push(Instruction::from_reader(reader)?);
                    reader.read_expected_byte(b':')?;
                }
                reader.read_expected_byte(b']')?;
                Ok(ByeOrSlimInstructions::Instructions(result))
            }
            b'b' => {
                reader.read_expected_byte(b'y')?;
                reader.read_expected_byte(b'e')?;
                Ok(ByeOrSlimInstructions::Bye)
            }
            other => {
                return Err(FromSlimReaderError::Other(
                    format!("Non expected byte {other}"),
                ))
            }
        }
    }
}

trait ReadByte {
    fn read_byte(&mut self) -> Result<u8, std::io::Error>;
    fn read_expected_byte(&mut self, expected_byte: u8) -> Result<(), std::io::Error> {
        let byte = self.read_byte()?;
        if byte == expected_byte {
            Ok(())
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Expected {expected_byte} but got {byte}",
            ));
        }
    }
}

impl<R> ReadByte for R
where
    R: BufRead,
{
    fn read_byte(&mut self) -> Result<u8, std::io::Error> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }
}

fn read_len(reader: &mut impl BufRead) -> Result<usize, std::io::Error> {
    let mut buffer = Vec::new();
    buffer.reserve_exact(6);
    reader.read_until(b':', &mut buffer)?;
    if buffer.len() < 6 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "Failure reading from Slim Server",
        ));
    }
    Ok(String::from_utf8_lossy(&buffer[..buffer.len() - 1])
        .parse()
        .map_err(|_| {
            std::io::Error::new(
                io::ErrorKind::InvalidData,
                "Failure converting read data to a number",
            )
        })?)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::error::Error;
    use std::io::Cursor;

    #[test]
    fn read_empty_string() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            String::new(),
            String::from_reader(&mut Cursor::new("000000:"))?
        );
        Ok(())
    }

    #[test]
    fn read_string() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            String::from("Hello world"),
            String::from_reader(&mut Cursor::new("000011:Hello world"))?
        );
        Ok(())
    }

    #[test]
    fn read_empty_vec() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            Vec::<String>::new(),
            Vec::<String>::from_reader(&mut Cursor::new("000009:[000000:]"))?
        );
        Ok(())
    }

    #[test]
    fn read_vec() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            vec!["Element1".to_string(), "Element2".into()],
            Vec::<String>::from_reader(&mut Cursor::new(
                "000041:[000002:000008:Element1:000008:Element2:]"
            ))?
        );
        Ok(())
    }

    #[test]
    fn read_instruction_result() -> Result<(), Box<dyn Error>> {
        let id = Id::from("01HFM0NQM3ZS6BBX0ZH6VA6DJX");
        assert_eq!(
            InstructionResult::Ok { id: id.clone() },
            InstructionResult::from_reader(&mut Cursor::new(
                "000053:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000002:OK:]"
            ))?
        );
        assert_eq!(
            InstructionResult::Void { id: id.clone() },
            InstructionResult::from_reader(&mut Cursor::new(
                "000061:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000010:/__VOID__/:]"
            ))?
        );
        assert_eq!(
            InstructionResult::String {
                id: id.clone(),
                value: "null".to_string()
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000055:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:null:]"
            ))?
        );
        assert_eq!(
            InstructionResult::String {
                id: id.clone(),
                value: "Value".into()
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000056:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000005:Value:]"
            ))?
        );
        assert_eq!(
            InstructionResult::Exception {
                id: id.clone(),
                message: ExceptionMessage::new("Message".into()),
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000073:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:0000021:__EXCEPTION__:Message:]"
            ))?
        );
        let exception = InstructionResult::from_reader(&mut Cursor::new(
            "000100:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:0000048:__EXCEPTION__:Some Exception message:<<Message>>:]"
        ))?;
        assert_eq!(
            InstructionResult::Exception {
                id: id.clone(),
                message: ExceptionMessage::new("Some Exception message:<<Message>>".into())
            },
            exception
        );
        let InstructionResult::Exception { id: _, message } = exception else {
            return Err("Expected exception".into());
        };
        assert_eq!("Message", message.pretty_message()?);
        Ok(())
    }
}
