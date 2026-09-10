use crate::{Instruction, InstructionResult, InstructionResultValue, SlimValue};

/// A complete SliM message. Its leading length is measured in UTF-8 bytes.
///
/// SliM uses a different unit for strings embedded in lists: UTF-16 code units.
/// Keeping the payload alongside the wire form makes that distinction explicit.
#[derive(PartialEq, Eq, Debug)]
pub struct SlimString {
    wire: Vec<u8>,
    payload: String,
}

impl SlimString {
    pub fn as_bytes(&self) -> &[u8] {
        &self.wire
    }

    fn from_payload(payload: String) -> Self {
        let wire = format!("{:06}:{}", payload.len(), payload).into_bytes();
        Self { wire, payload }
    }

    fn nested(&self) -> String {
        format!("{:06}:{}", utf16_len(&self.payload), self.payload)
    }
}

pub trait ToSlimString {
    fn to_slim_string(&self) -> SlimString;
}

impl ToSlimString for &str {
    fn to_slim_string(&self) -> SlimString {
        SlimString::from_payload((*self).into())
    }
}

impl ToSlimString for String {
    fn to_slim_string(&self) -> SlimString {
        self.as_str().to_slim_string()
    }
}

impl ToSlimString for SlimValue {
    fn to_slim_string(&self) -> SlimString {
        match self {
            Self::String(value) => value.to_slim_string(),
            Self::List(values) => values.to_slim_string(),
        }
    }
}

impl<T> ToSlimString for Vec<T>
where
    T: ToSlimString,
{
    fn to_slim_string(&self) -> SlimString {
        self.as_slice().to_slim_string()
    }
}

impl<T> ToSlimString for &[T]
where
    T: ToSlimString,
{
    fn to_slim_string(&self) -> SlimString {
        let mut payload = format!("[{:06}:", self.len());
        for value in *self {
            payload.push_str(&value.to_slim_string().nested());
            payload.push(':');
        }
        payload.push(']');
        SlimString::from_payload(payload)
    }
}

impl<T, const S: usize> ToSlimString for [T; S]
where
    T: ToSlimString,
{
    fn to_slim_string(&self) -> SlimString {
        self.as_slice().to_slim_string()
    }
}

impl ToSlimString for Instruction {
    fn to_slim_string(&self) -> SlimString {
        match self {
            Self::Malformed { fields, .. } => fields.to_slim_string(),
            Self::Import { id, path } => [id.0.as_str(), "import", path.as_str()].to_slim_string(),
            Self::Make {
                id,
                instance,
                class,
                args,
            } => instruction_parts(
                [id.0.as_str(), "make", instance.as_str(), class.as_str()],
                args,
            ),
            Self::Call {
                id,
                instance,
                function,
                args,
            } => instruction_parts(
                [id.0.as_str(), "call", instance.as_str(), function.as_str()],
                args,
            ),
            Self::Assign { id, symbol, value } => {
                [id.0.as_str(), "assign", symbol.as_str(), value.as_str()].to_slim_string()
            }
            Self::CallAndAssign {
                id,
                symbol,
                instance,
                function,
                args,
            } => instruction_parts(
                [
                    id.0.as_str(),
                    "callAndAssign",
                    symbol.as_str(),
                    instance.as_str(),
                    function.as_str(),
                ],
                args,
            ),
        }
    }
}

fn instruction_parts<const N: usize>(head: [&str; N], args: &[String]) -> SlimString {
    let mut values = head.into_iter().map(str::to_owned).collect::<Vec<_>>();
    values.extend(args.iter().cloned());
    values.to_slim_string()
}

impl ToSlimString for Box<dyn ToSlimString> {
    fn to_slim_string(&self) -> SlimString {
        self.as_ref().to_slim_string()
    }
}

impl ToSlimString for InstructionResult {
    fn to_slim_string(&self) -> SlimString {
        vec![self.id.0.clone(), self.value.to_slim_string().payload].to_slim_string()
    }
}

impl ToSlimString for InstructionResultValue {
    fn to_slim_string(&self) -> SlimString {
        match self {
            Self::Ok => "OK".to_slim_string(),
            Self::Void => "/__VOID__/".to_slim_string(),
            Self::String(value) => value.to_slim_string(),
            Self::Exception(message) => {
                format!("__EXCEPTION__:{}", message.raw_message()).to_slim_string()
            }
            Self::List(list) => list.to_slim_string(),
        }
    }
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Id;

    #[test]
    fn outer_frames_use_utf8_bytes_and_nested_values_use_utf16_units() {
        assert_eq!(b"000002:\xC3\xA9", "é".to_slim_string().as_bytes());
        assert_eq!(
            b"000019:[000001:000001:\xC3\xA9:]",
            ["é"].to_slim_string().as_bytes()
        );
        assert_eq!(
            b"000021:[000001:000002:\xF0\x9F\x98\x80:]",
            ["😀"].to_slim_string().as_bytes()
        );
    }

    #[test]
    fn nested_lists_are_encoded_recursively() {
        let nested = vec![vec!["one", "two"], vec!["😀"]];
        assert_eq!(
            b"000077:[000002:000031:[000002:000003:one:000003:two:]:000019:[000001:000002:\xF0\x9F\x98\x80:]:]",
            nested.to_slim_string().as_bytes()
        );
    }

    #[test]
    fn recursive_wire_values_round_trip_through_their_encoding() {
        let value = SlimValue::List(vec![
            SlimValue::String("é".into()),
            SlimValue::List(vec![SlimValue::String("😀".into())]),
        ]);
        assert_eq!(
            b"000048:[000002:000001:\xC3\xA9:000019:[000001:000002:\xF0\x9F\x98\x80:]:]",
            value.to_slim_string().as_bytes()
        );
    }

    #[test]
    fn lengths_grow_past_six_digits_without_truncation() {
        let value = "x".repeat(1_000_000);
        assert!(value.to_slim_string().as_bytes().starts_with(b"1000000:"));
    }

    #[test]
    fn instruction_serialization_remains_available() {
        let id = Id::from("01HFM0NQM3ZS6BBX0ZH6VA6DJX");
        let instruction = Instruction::Call {
            id,
            instance: "fixture".into(),
            function: "answer".into(),
            args: vec!["😀".into()],
        };
        assert_eq!(
            b"000096:[000005:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:call:000007:fixture:000006:answer:000002:\xF0\x9F\x98\x80:]",
            instruction.to_slim_string().as_bytes()
        );
    }
}
