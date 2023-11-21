use anyhow::{anyhow, bail, Result};
use std::io::BufRead;

use super::{Id, InstructionResult};

pub trait FromSlimReader {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self>
    where
        Self: Sized;
}

impl FromSlimReader for String {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self> {
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
    fn from_reader(reader: &mut impl BufRead) -> Result<Self> {
        let result = Vec::from_reader(reader)?;
        result
            .try_into()
            .map_err(|_| anyhow!("Missing elements from array"))
    }
}

impl<T> FromSlimReader for Vec<T>
where
    T: FromSlimReader,
{
    fn from_reader(reader: &mut impl BufRead) -> Result<Self> {
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
    fn from_reader(reader: &mut impl BufRead) -> Result<Self> {
        let [id, value] = <[String; 2]>::from_reader(reader)?;
        let id = Id::from_string(id)?;
        Ok(match value.as_str() {
            "OK" => InstructionResult::Ok { id },
            "null" => InstructionResult::Null { id },
            _ => InstructionResult::String { id, value },
        })
    }
}

trait ReadByte {
    fn read_byte(&mut self) -> Result<u8>;
    fn read_expected_byte(&mut self, expected_byte: u8) -> Result<()> {
        let byte = self.read_byte()?;
        if byte == expected_byte {
            Ok(())
        } else {
            bail!("Expected {expected_byte} but got {byte}")
        }
    }
}

impl<R> ReadByte for R
where
    R: BufRead,
{
    fn read_byte(&mut self) -> Result<u8> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }
}

fn read_len(reader: &mut impl BufRead) -> Result<usize> {
    let mut buffer = Vec::new();
    buffer.reserve_exact(6);
    reader.read_until(b':', &mut buffer)?;
    if buffer.len() < 6 {
        bail!("Failure reading from Slim Server");
    }
    Ok(String::from_utf8_lossy(&buffer[..buffer.len() - 1]).parse()?)
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use super::*;
    use anyhow::Result;

    #[test]
    fn read_empty_string() -> Result<()> {
        assert_eq!(
            String::new(),
            String::from_reader(&mut Cursor::new("000000:"))?
        );
        Ok(())
    }

    #[test]
    fn read_string() -> Result<()> {
        assert_eq!(
            String::from("Hello world"),
            String::from_reader(&mut Cursor::new("000011:Hello world"))?
        );
        Ok(())
    }

    #[test]
    fn read_empty_vec() -> Result<()> {
        assert_eq!(
            Vec::<String>::new(),
            Vec::<String>::from_reader(&mut Cursor::new("000009:[000000:]"))?
        );
        Ok(())
    }

    #[test]
    fn read_vec() -> Result<()> {
        assert_eq!(
            vec!["Element1".to_string(), "Element2".into()],
            Vec::<String>::from_reader(&mut Cursor::new(
                "000041:[000002:000008:Element1:000008:Element2:]"
            ))?
        );
        Ok(())
    }
}
