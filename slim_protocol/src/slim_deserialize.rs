use std::io::{self, BufRead, Cursor};

use thiserror::Error;

use crate::{
    ByeOrSlimInstructions, ExceptionMessage, Id, Instruction, InstructionResult,
    InstructionResultValue, SlimValue,
};

#[derive(Debug, Error)]
pub enum FromSlimReaderError {
    #[error(transparent)]
    IoError(#[from] io::Error),
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
        read_outer_payload(reader)
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
        let payload = read_outer_payload(reader)?;
        parse_list_payload(&payload)?
            .into_iter()
            .map(|item| T::from_reader(&mut Cursor::new(outer_frame(item))))
            .collect()
    }
}

impl FromSlimReader for SlimValue {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        parse_slim_value(read_outer_payload(reader)?)
    }
}

impl FromSlimReader for InstructionResultValue {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        parse_instruction_result_value(read_outer_payload(reader)?)
    }
}

impl FromSlimReader for InstructionResult {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        let values = parse_list_payload(&read_outer_payload(reader)?)?;
        instruction_result_from_values(values)
    }
}

impl FromSlimReader for Instruction {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        instruction_from_values(parse_list_payload(&read_outer_payload(reader)?)?)
    }
}

impl FromSlimReader for ByeOrSlimInstructions {
    fn from_reader(reader: &mut impl BufRead) -> Result<Self, FromSlimReaderError> {
        let payload = read_outer_payload(reader)?;
        if payload == "bye" {
            return Ok(Self::Bye);
        }

        let instructions = parse_list_payload(&payload)?
            .into_iter()
            .map(|instruction| parse_list_payload(&instruction).and_then(instruction_from_values))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::Instructions(instructions))
    }
}

fn instruction_result_from_values(
    values: Vec<String>,
) -> Result<InstructionResult, FromSlimReaderError> {
    let [id, value]: [String; 2] = values.try_into().map_err(|_| {
        FromSlimReaderError::Other("Expected an instruction result with an id and a value".into())
    })?;
    Ok(InstructionResult {
        id: Id::from(id),
        value: parse_instruction_result_value(value)?,
    })
}

fn parse_instruction_result_value(
    value: String,
) -> Result<InstructionResultValue, FromSlimReaderError> {
    Ok(match parse_slim_value(value)? {
        SlimValue::List(values) => InstructionResultValue::List(
            values
                .into_iter()
                .map(slim_value_to_instruction_result_value)
                .collect(),
        ),
        SlimValue::String(value) if value == "OK" => InstructionResultValue::Ok,
        SlimValue::String(value) if value == "/__VOID__/" => InstructionResultValue::Void,
        SlimValue::String(value) => {
            if let Some(message) = value.strip_prefix("__EXCEPTION__:") {
                InstructionResultValue::Exception(ExceptionMessage::new(message.into()))
            } else {
                InstructionResultValue::String(value)
            }
        }
    })
}

fn slim_value_to_instruction_result_value(value: SlimValue) -> InstructionResultValue {
    match value {
        SlimValue::List(values) => InstructionResultValue::List(
            values
                .into_iter()
                .map(slim_value_to_instruction_result_value)
                .collect(),
        ),
        SlimValue::String(value) => InstructionResultValue::String(value),
    }
}

fn parse_slim_value(value: String) -> Result<SlimValue, FromSlimReaderError> {
    if value.starts_with('[') {
        if let Ok(values) = parse_list_payload(&value) {
            return Ok(SlimValue::List(
                values
                    .into_iter()
                    .map(parse_slim_value)
                    .collect::<Result<_, _>>()?,
            ));
        }
    }
    Ok(SlimValue::String(value))
}

fn instruction_from_values(fields: Vec<String>) -> Result<Instruction, FromSlimReaderError> {
    let id = Id::from(fields.first().cloned().unwrap_or_default());
    let malformed = || Instruction::Malformed {
        id: id.clone(),
        fields: fields.clone(),
    };
    let Some(operation) = fields.get(1).map(String::as_str) else {
        return Ok(malformed());
    };

    match operation {
        "import" if fields.len() == 3 => Ok(Instruction::Import {
            id,
            path: fields[2].clone(),
        }),
        "make" if fields.len() >= 4 => Ok(Instruction::Make {
            id,
            instance: fields[2].clone(),
            class: fields[3].clone(),
            args: fields[4..]
                .iter()
                .cloned()
                .map(parse_slim_value)
                .collect::<Result<_, _>>()?,
        }),
        "call" if fields.len() >= 4 => Ok(Instruction::Call {
            id,
            instance: fields[2].clone(),
            function: fields[3].clone(),
            args: fields[4..]
                .iter()
                .cloned()
                .map(parse_slim_value)
                .collect::<Result<_, _>>()?,
        }),
        "callAndAssign" if fields.len() >= 5 => Ok(Instruction::CallAndAssign {
            id,
            symbol: fields[2].clone(),
            instance: fields[3].clone(),
            function: fields[4].clone(),
            args: fields[5..]
                .iter()
                .cloned()
                .map(parse_slim_value)
                .collect::<Result<_, _>>()?,
        }),
        "assign" if fields.len() == 4 => Ok(Instruction::Assign {
            id,
            symbol: fields[2].clone(),
            value: parse_slim_value(fields[3].clone())?,
        }),
        _ => Ok(malformed()),
    }
}

/// Reads a complete message. The outer length is counted in UTF-8 bytes.
fn read_outer_payload(reader: &mut impl BufRead) -> Result<String, FromSlimReaderError> {
    let length = read_length_prefix(reader)?;
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes)
        .map_err(|_| FromSlimReaderError::Other("Message payload is not valid UTF-8".into()))
}

fn outer_frame(payload: String) -> Vec<u8> {
    format!("{:06}:{}", payload.len(), payload).into_bytes()
}

fn read_length_prefix(reader: &mut impl BufRead) -> Result<usize, FromSlimReaderError> {
    let mut prefix = Vec::new();
    reader.read_until(b':', &mut prefix)?;
    if prefix.last() != Some(&b':') {
        return Err(FromSlimReaderError::Other(
            "Missing message length terminator".into(),
        ));
    }
    parse_length(&prefix[..prefix.len() - 1])
}

fn parse_list_payload(payload: &str) -> Result<Vec<String>, FromSlimReaderError> {
    let mut position = 0;
    expect_byte(payload, &mut position, b'[')?;
    let count = parse_length_at(payload, &mut position)?;
    let mut values = Vec::new();
    for _ in 0..count {
        values.push(parse_utf16_string(payload, &mut position)?);
        expect_byte(payload, &mut position, b':')?;
    }
    expect_byte(payload, &mut position, b']')?;
    if position != payload.len() {
        return Err(FromSlimReaderError::Other(
            "Trailing data after SliM list terminator".into(),
        ));
    }
    Ok(values)
}

fn parse_utf16_string(payload: &str, position: &mut usize) -> Result<String, FromSlimReaderError> {
    let length = parse_length_at(payload, position)?;
    if length == 0 {
        return Ok(String::new());
    }
    let start = *position;
    let mut utf16_units = 0;
    for (offset, character) in payload[start..].char_indices() {
        utf16_units += character.len_utf16();
        if utf16_units == length {
            let end = start + offset + character.len_utf8();
            *position = end;
            return Ok(payload[start..end].into());
        }
        if utf16_units > length {
            return Err(FromSlimReaderError::Other(
                "SliM string length ends in the middle of a UTF-16 character".into(),
            ));
        }
    }
    Err(FromSlimReaderError::Other(
        "SliM string is shorter than its declared UTF-16 length".into(),
    ))
}

fn parse_length_at(payload: &str, position: &mut usize) -> Result<usize, FromSlimReaderError> {
    let rest = &payload[*position..];
    let Some(end) = rest.find(':') else {
        return Err(FromSlimReaderError::Other(
            "Missing SliM length terminator".into(),
        ));
    };
    let prefix = &rest[..end];
    *position += end + 1;
    parse_length(prefix.as_bytes())
}

fn parse_length(prefix: &[u8]) -> Result<usize, FromSlimReaderError> {
    if prefix.len() < 6 || !prefix.iter().all(u8::is_ascii_digit) {
        return Err(FromSlimReaderError::Other(
            "SliM lengths must contain at least six ASCII digits".into(),
        ));
    }
    std::str::from_utf8(prefix)
        .expect("ASCII digits are valid UTF-8")
        .parse()
        .map_err(|_| FromSlimReaderError::Other("SliM length is out of range".into()))
}

fn expect_byte(
    payload: &str,
    position: &mut usize,
    expected: u8,
) -> Result<(), FromSlimReaderError> {
    match payload.as_bytes().get(*position) {
        Some(actual) if *actual == expected => {
            *position += 1;
            Ok(())
        }
        Some(actual) => Err(FromSlimReaderError::Other(format!(
            "Expected {} but got {}",
            expected as char, *actual as char
        ))),
        None => Err(FromSlimReaderError::Other(format!(
            "Expected {} but reached the end of the SliM list",
            expected as char
        ))),
    }
}

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::io::{Cursor, Read};

    use super::*;
    use crate::ToSlimString;

    #[test]
    fn reads_unicode_using_utf16_lengths() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            vec!["é".to_string(), "😀".into()],
            Vec::<String>::from_reader(&mut Cursor::new("000031:[000002:000001:é:000002:😀:]"))?
        );
        Ok(())
    }

    #[test]
    fn reads_empty_strings_and_rejects_partial_surrogate_lengths() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            vec![String::new()],
            Vec::<String>::from_reader(&mut Cursor::new("000017:[000001:000000::]"))?
        );
        let error = Vec::<String>::from_reader(&mut Cursor::new("000021:[000001:000001:😀:]"))
            .expect_err("an astral character needs two UTF-16 code units");
        assert!(error.to_string().contains("middle of a UTF-16 character"));
        Ok(())
    }

    #[test]
    fn rejects_wrong_list_size_and_terminators() {
        Vec::<String>::from_reader(&mut Cursor::new("000018:[000002:000001:a:]"))
            .expect_err("list count must match");

        let err = Vec::<String>::from_reader(&mut Cursor::new("000018:[000001:000001:a;]"))
            .expect_err("item terminator must be a colon")
            .to_string();
        assert_eq!("Expected : but got ;", err);
    }

    #[test]
    fn recursive_result_lists_round_trip() -> Result<(), Box<dyn Error>> {
        let result = InstructionResult {
            id: Id::from("id"),
            value: InstructionResultValue::List(vec![
                InstructionResultValue::String("é".into()),
                InstructionResultValue::List(vec![InstructionResultValue::String("😀".into())]),
            ]),
        };
        let wire = result.to_slim_string();
        assert_eq!(
            result,
            InstructionResult::from_reader(&mut Cursor::new(wire.as_bytes()))?
        );
        Ok(())
    }

    #[test]
    fn recursive_wire_values_round_trip() -> Result<(), Box<dyn Error>> {
        let value = SlimValue::List(vec![
            SlimValue::String("é".into()),
            SlimValue::List(vec![SlimValue::String("😀".into())]),
        ]);
        let wire = value.to_slim_string();
        assert_eq!(
            value,
            SlimValue::from_reader(&mut Cursor::new(wire.as_bytes()))?
        );
        Ok(())
    }

    #[test]
    fn fragmented_reads_work() -> Result<(), Box<dyn Error>> {
        struct OneByteReader(Cursor<Vec<u8>>);
        impl Read for OneByteReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let mut byte = [0];
                let read = self.0.read(&mut byte)?;
                if read == 0 {
                    return Ok(0);
                }
                buffer[0] = byte[0];
                Ok(1)
            }
        }
        let input = vec!["😀".to_string()].to_slim_string().as_bytes().to_vec();
        let mut reader = io::BufReader::new(OneByteReader(Cursor::new(input)));
        assert_eq!(
            vec!["😀".to_string()],
            Vec::<String>::from_reader(&mut reader)?
        );
        Ok(())
    }

    #[test]
    fn malformed_instruction_retains_its_id_and_raw_fields() -> Result<(), Box<dyn Error>> {
        let message = vec![
            vec!["unknown_id", "unknown", "field"],
            vec!["short_id", "call", "fixture"],
        ]
        .to_slim_string();
        assert_eq!(
            ByeOrSlimInstructions::Instructions(vec![
                Instruction::Malformed {
                    id: Id::from("unknown_id"),
                    fields: vec!["unknown_id".into(), "unknown".into(), "field".into()],
                },
                Instruction::Malformed {
                    id: Id::from("short_id"),
                    fields: vec!["short_id".into(), "call".into(), "fixture".into()],
                },
            ]),
            ByeOrSlimInstructions::from_reader(&mut Cursor::new(message.as_bytes()))?
        );
        Ok(())
    }

    #[test]
    fn all_instruction_forms_round_trip() -> Result<(), Box<dyn Error>> {
        let instructions = vec![
            Instruction::Import {
                id: Id::from("import"),
                path: "Fixtures".into(),
            },
            Instruction::Make {
                id: Id::from("make"),
                instance: "fixture".into(),
                class: "Calculator".into(),
                args: vec!["1".into()],
            },
            Instruction::Call {
                id: Id::from("call"),
                instance: "fixture".into(),
                function: "value".into(),
                args: vec!["é".into()],
            },
            Instruction::CallAndAssign {
                id: Id::from("call-and-assign"),
                symbol: "answer".into(),
                instance: "fixture".into(),
                function: "answer".into(),
                args: vec!["😀".into()],
            },
            Instruction::Assign {
                id: Id::from("assign"),
                symbol: "value".into(),
                value: SlimValue::List(vec!["42".into()]),
            },
        ];
        let wire = instructions.to_slim_string();
        assert_eq!(
            ByeOrSlimInstructions::Instructions(instructions),
            ByeOrSlimInstructions::from_reader(&mut Cursor::new(wire.as_bytes()))?
        );
        Ok(())
    }

    #[test]
    fn instruction_arguments_preserve_nested_lists() -> Result<(), Box<dyn Error>> {
        let instruction = Instruction::Call {
            id: Id::from("nested"),
            instance: "fixture".into(),
            function: "accept".into(),
            args: vec![SlimValue::List(vec![
                SlimValue::String("one".into()),
                SlimValue::List(vec![SlimValue::String("two".into())]),
            ])],
        };
        let wire = instruction.to_slim_string();
        assert_eq!(
            instruction,
            Instruction::from_reader(&mut Cursor::new(wire.as_bytes()))?
        );
        Ok(())
    }

    #[test]
    fn structurally_invalid_instruction_list_is_a_wire_error() {
        let malformed = ["[000002:000002:id;]"].to_slim_string();
        ByeOrSlimInstructions::from_reader(&mut Cursor::new(malformed.as_bytes()))
            .expect_err("invalid nested list syntax must fail at the wire layer");
    }

    #[test]
    fn a_string_starting_with_a_bracket_is_not_always_a_list() -> Result<(), Box<dyn Error>> {
        let value = SlimValue::String("[literal text".into());
        let wire = value.to_slim_string();
        assert_eq!(
            value,
            SlimValue::from_reader(&mut Cursor::new(wire.as_bytes()))?
        );
        Ok(())
    }

    #[test]
    fn recursive_response_lists_keep_control_markers_as_strings() -> Result<(), Box<dyn Error>> {
        let response = InstructionResult {
            id: Id::from("id"),
            value: InstructionResultValue::List(vec![
                InstructionResultValue::String("OK".into()),
                InstructionResultValue::String("/__VOID__/".into()),
                InstructionResultValue::String("__EXCEPTION__:text".into()),
                InstructionResultValue::List(vec![InstructionResultValue::String("OK".into())]),
            ]),
        };
        let wire = response.to_slim_string();
        assert_eq!(
            response,
            InstructionResult::from_reader(&mut Cursor::new(wire.as_bytes()))?
        );
        Ok(())
    }

    #[test]
    fn top_level_response_markers_keep_their_protocol_meaning() -> Result<(), Box<dyn Error>> {
        let results = vec![
            InstructionResult::ok(Id::from("ok")),
            InstructionResult::void(Id::from("void")),
            InstructionResult::exception(
                Id::from("exception"),
                ExceptionMessage::new("message:<<failure>>".into()),
            ),
        ];
        let wire = results.to_slim_string();
        assert_eq!(
            results,
            Vec::<InstructionResult>::from_reader(&mut Cursor::new(wire.as_bytes()))?
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_outer_frames_and_trailing_list_data() {
        for wire in [
            b"0000:x".as_slice(),
            b"00000x:x".as_slice(),
            b"000006".as_slice(),
            b"000001:\xff".as_slice(),
        ] {
            String::from_reader(&mut Cursor::new(wire)).expect_err("invalid frame must fail");
        }

        Vec::<String>::from_reader(&mut Cursor::new("000010:[000000:]x"))
            .expect_err("trailing data after a list must fail");
    }

    #[test]
    fn accepts_seven_digit_outer_lengths() -> Result<(), Box<dyn Error>> {
        let value = "x".repeat(1_000_000);
        assert_eq!(
            value,
            String::from_reader(&mut Cursor::new(value.to_slim_string().as_bytes()))?
        );
        Ok(())
    }
}
