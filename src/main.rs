use crate::tokenizer::Token;
use anyhow::{anyhow, bail, Result};
use clap::Parser;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::{iter::Peekable, str::Chars};
use tokenizer::Tokenizer;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of times to greet
    // #[arg(short, long)]
    // slim_server: String,
    #[arg(short, long)]
    port: u16,
}

mod tokenizer;

fn main() -> Result<()> {
    let args = Args::parse();
    // let mut slim_server = Command::new("sh").arg("-c").arg(args.slim_server).spawn()?;

    let tcp_stream = TcpStream::connect(format!("127.0.0.1:{}", args.port))?;
    let mut connection = SlimConnection::new(tcp_stream.try_clone()?, tcp_stream)?;
    dbg!(connection.send_instructions(&[Instruction::Import {
        id: Id("import_0_0".into()),
        path: "Fixtures".into(),
    }])?);

    println!("{:?}", connection.version);

    println!("Hello, world!");
    // slim_server.kill()?;
    Ok(())
}

struct SlimConnection<R, W> {
    reader: BufReader<R>,
    writer: W,
    version: SlimVersion,
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
            version,
        })
    }

    pub fn send_instructions(&mut self, data: &[Instruction]) -> Result<Vec<InstructionResult>> {
        self.writer.write_all(data.to_slim_string().as_bytes())?;
        Ok(Vec::from_reader(&mut self.reader)?)
    }
}

#[derive(Debug)]
enum SlimVersion {
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

#[derive(Debug, PartialEq, Eq)]
struct Id(String);

#[derive(Debug, PartialEq, Eq)]
enum Instruction {
    Import {
        id: Id,
        path: String,
    },
    Make {
        id: Id,
        instance: String,
        class: String,
        args: String,
    },
    Call {
        id: Id,
        instance: String,
        function: String,
        args: String,
    },
    CallAndAssign {
        id: Id,
        symbol: String,
        instance: String,
        function: String,
        args: Vec<String>,
    },
    Assign {
        id: Id,
        symbol: String,
        value: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
enum InstructionResult {
    Ok { id: Id },
    String { id: Id, value: String },
}

impl FromSlimReader for InstructionResult {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self> {
        let [id, value] = <[String; 2]>::from_reader(reader)?;
        let id = Id(id);
        Ok(match value.as_str() {
            "OK" => InstructionResult::Ok { id },
            _ => InstructionResult::String { id, value },
        })
    }
}

trait ToSlimString {
    fn to_slim_string(&self) -> String;
}

impl ToSlimString for String {
    fn to_slim_string(&self) -> String {
        format!("{:0>6}:{}", self.len(), self)
    }
}

impl<'a> ToSlimString for &'a str {
    fn to_slim_string(&self) -> String {
        format!("{:0>6}:{}", self.len(), self)
    }
}

impl<'a, T> ToSlimString for &'a [T]
where
    T: ToSlimString,
{
    fn to_slim_string(&self) -> String {
        let mut result = String::from("[");
        result += &format!("{:0>6}:", self.len());
        for value in self.into_iter() {
            result += &value.to_slim_string();
            result += ":";
        }
        result += "]";
        result.to_slim_string()
    }
}

impl<T, const S: usize> ToSlimString for [T; S]
where
    T: ToSlimString,
{
    fn to_slim_string(&self) -> String {
        let mut result = String::from("[");
        result += &format!("{:0>6}:", self.len());
        for value in self.into_iter() {
            result += &value.to_slim_string();
            result += ":";
        }
        result += "]";
        result.to_slim_string()
    }
}

impl ToSlimString for Instruction {
    fn to_slim_string(&self) -> String {
        match self {
            Self::Import { id, path } => [id.0.as_str(), "import", path.as_str()].to_slim_string(),
            _ => todo!(),
        }
    }
}

trait FromSlimReader {
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
        Ok(result
            .try_into()
            .map_err(|_| anyhow!("Missing elements from array"))?)
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

fn read_len(reader: &mut impl BufRead) -> Result<usize> {
    let mut buffer = Vec::new();
    buffer.reserve_exact(6);
    reader.read_until(b':', &mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer[..buffer.len() - 1]).parse()?)
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
