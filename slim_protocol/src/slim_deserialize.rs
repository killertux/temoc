use read_char::read_next_char;
use std::io::{self, BufRead, Cursor};
use thiserror::Error;

use crate::{ExceptionMessage, Instruction, InstructionResultValue};

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
        let string = reader.read_n_chars(len)?;
        Ok(string)
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
        reader.read_expected_char('[')?;
        let n_elements = read_len(reader)?;
        for _ in 0..n_elements {
            result.push(T::from_reader(reader)?);
            reader.read_expected_char(':')?;
        }
        reader.read_expected_char(']')?;
        Ok(result)
    }
}

impl FromSlimReader for InstructionResultValue {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        let value = String::from_reader(reader)?;
        Ok(match value.as_str() {
            "OK" => InstructionResultValue::Ok,
            "/__VOID__/" => InstructionResultValue::Void,
            value if value.starts_with('[') => {
                let values: Vec<InstructionResultValue> =
                    Vec::from_reader(&mut Cursor::new(value))?;
                InstructionResultValue::List(values)
            }
            other => {
                if let Some(message) = other.strip_prefix("__EXCEPTION__:") {
                    InstructionResultValue::Exception(ExceptionMessage::new(message.into()))
                } else {
                    InstructionResultValue::String(other.into())
                }
            }
        })
    }
}

impl FromSlimReader for InstructionResult {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        let [id, value] = <[String; 2]>::from_reader(reader)?;
        let id = Id::from(id);
        Ok(InstructionResult {
            id,
            value: match value.as_str() {
                "OK" => InstructionResultValue::Ok,
                "/__VOID__/" => InstructionResultValue::Void,
                value if value.starts_with('[') => {
                    let value = format!("{:0>6}:{}", value.len(), value);
                    let values: Vec<InstructionResultValue> =
                        Vec::from_reader(&mut Cursor::new(value))?;
                    InstructionResultValue::List(values)
                }
                other => {
                    if let Some(message) = other.strip_prefix("__EXCEPTION__:") {
                        InstructionResultValue::Exception(ExceptionMessage::new(message.into()))
                    } else {
                        InstructionResultValue::String(other.into())
                    }
                }
            },
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
        match reader.read_char()? {
            '[' => {
                let mut result = Vec::new();
                let n_elements = read_len(reader)?;
                for _ in 0..n_elements {
                    result.push(Instruction::from_reader(reader)?);
                    reader.read_expected_char(':')?;
                }
                reader.read_expected_char(']')?;
                Ok(ByeOrSlimInstructions::Instructions(result))
            }
            'b' => {
                reader.read_expected_char('y')?;
                reader.read_expected_char('e')?;
                Ok(ByeOrSlimInstructions::Bye)
            }
            other => Err(FromSlimReaderError::Other(format!(
                "Non expected byte {other}"
            ))),
        }
    }
}

trait ReadChar {
    fn read_char(&mut self) -> Result<char, std::io::Error>;
    fn read_n_chars(&mut self, n: usize) -> Result<String, std::io::Error> {
        let mut buffer = String::new();
        buffer.reserve(n);
        for _ in 0..n {
            buffer.push(self.read_char()?);
        }
        Ok(buffer)
    }
    fn read_expected_char(&mut self, expected_char: char) -> Result<(), std::io::Error> {
        let char = self.read_char()?;
        if char == expected_char {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Expected {expected_char} but got {char}"),
            ))
        }
    }
}

impl<R> ReadChar for R
where
    R: BufRead,
{
    fn read_char(&mut self) -> Result<char, std::io::Error> {
        read_next_char(self).map_err(|err| match err {
            read_char::Error::NotAnUtf8(_) => {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Non UTF-8 character")
            }
            read_char::Error::Io(err) => err,
            read_char::Error::EOF => std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Unexpected end of stream",
            ),
        })
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
    String::from_utf8_lossy(&buffer[..buffer.len() - 1])
        .parse()
        .map_err(|_| {
            std::io::Error::new(
                io::ErrorKind::InvalidData,
                "Failure converting read data to a number",
            )
        })
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
    fn read_string_with_special_chars() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            String::from("Tipo de evento inválido"),
            String::from_reader(&mut Cursor::new("000023:Tipo de evento inválido"))?
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
    fn read_with_incorrect_values() -> Result<(), Box<dyn Error>> {
        let err = Vec::<String>::from_reader(&mut Cursor::new("000009:[000000:A]"))
            .expect_err("Expected error")
            .to_string();
        assert_eq!("Expected ] but got A", err);
        Ok(())
    }

    #[test]
    fn read_instruction_result() -> Result<(), Box<dyn Error>> {
        let id = Id::from("01HFM0NQM3ZS6BBX0ZH6VA6DJX");
        assert_eq!(
            InstructionResult {
                id: id.clone(),
                value: InstructionResultValue::Ok
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000053:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000002:OK:]"
            ))?
        );
        assert_eq!(
            InstructionResult {
                id: id.clone(),
                value: InstructionResultValue::Void
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000061:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000010:/__VOID__/:]"
            ))?
        );
        assert_eq!(
            InstructionResult {
                id: id.clone(),
                value: InstructionResultValue::String("null".to_string()),
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000055:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:null:]"
            ))?
        );
        assert_eq!(
            InstructionResult {
                id: id.clone(),
                value: InstructionResultValue::String("Value".to_string()),
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000056:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000005:Value:]"
            ))?
        );
        assert_eq!(
            InstructionResult {
                id: id.clone(),
                value: InstructionResultValue::List(vec![InstructionResultValue::String("Value 1".to_string()), InstructionResultValue::String("Value 2".to_string())]),
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000056:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000039:[000002:000007:Value 1:000007:Value 2:]:]"
            ))?
        );
        assert_eq!(
            InstructionResult {
                id: id.clone(),
                value: InstructionResultValue::Exception(ExceptionMessage::new("Message".into())),
            },
            InstructionResult::from_reader(&mut Cursor::new(
                "000073:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:0000021:__EXCEPTION__:Message:]"
            ))?
        );
        let exception = InstructionResult::from_reader(&mut Cursor::new(
            "000100:[000002:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:0000048:__EXCEPTION__:Some Exception message:<<Message>>:]"
        ))?;
        assert_eq!(
            InstructionResult {
                id: id.clone(),
                value: InstructionResultValue::Exception(ExceptionMessage::new(
                    "Some Exception message:<<Message>>".into()
                ))
            },
            exception
        );
        let InstructionResultValue::Exception(message) = exception.value else {
            return Err("Expected exception".into());
        };
        assert_eq!("Message", message.pretty_message()?);
        Ok(())
    }
}
