use crate::InstructionResult;

use super::Instruction;

#[derive(PartialEq, Eq, Debug)]
pub struct SlimString(String);

impl SlimString {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

pub trait ToSlimString {
    fn to_slim_string(&self) -> SlimString;
}

impl<'a> ToSlimString for &'a str {
    fn to_slim_string(&self) -> SlimString {
        SlimString(format!("{:0>6}:{}", self.len(), self))
    }
}

impl ToSlimString for String {
    fn to_slim_string(&self) -> SlimString {
        self.as_str().to_slim_string()
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

impl<'a, T> ToSlimString for &'a [T]
where
    T: ToSlimString,
{
    fn to_slim_string(&self) -> SlimString {
        let mut result = String::from("[");
        result += &format!("{:0>6}:", self.len());
        for value in self.iter() {
            result += &value.to_slim_string().0;
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
    fn to_slim_string(&self) -> SlimString {
        self.as_slice().to_slim_string()
    }
}

impl ToSlimString for Instruction {
    fn to_slim_string(&self) -> SlimString {
        match self {
            Self::Import { id, path } => [id.0.as_str(), "import", path.as_str()].to_slim_string(),
            Self::Make {
                id,
                instance,
                class,
                args,
            } => [
                [id.0.as_str(), "make", instance, class].as_slice(),
                into_vec_str(args).as_slice(),
            ]
            .concat()
            .to_slim_string(),
            Self::Call {
                id,
                instance,
                function,
                args,
            } => [
                [id.0.as_str(), "call", instance, function].as_slice(),
                into_vec_str(args).as_slice(),
            ]
            .concat()
            .to_slim_string(),
            Self::Assign { id, symbol, value } => {
                [id.0.as_str(), "assign", symbol, value].to_slim_string()
            }
            Self::CallAndAssign {
                id,
                symbol,
                instance,
                function,
                args,
            } => [
                [id.0.as_str(), "callAndAssign", symbol, instance, function].as_slice(),
                into_vec_str(args).as_slice(),
            ]
            .concat()
            .to_slim_string(),
        }
    }
}

impl ToSlimString for InstructionResult {
    fn to_slim_string(&self) -> SlimString {
        match self {
            InstructionResult::Ok { id } => [id.0.as_str(), "OK"].to_slim_string(),
            InstructionResult::Void { id } => [id.0.as_str(), "/__VOID__/"].to_slim_string(),
            InstructionResult::String { id, value } => {
                [id.0.as_str(), value.as_str()].to_slim_string()
            }
            InstructionResult::Exception { id, message } => [
                id.0.as_str(),
                format!("__EXCEPTION__:{}", message.raw_message()).as_str(),
            ]
            .to_slim_string(),
        }
    }
}

fn into_vec_str(args: &[String]) -> Vec<&str> {
    args.iter().map(|string| string.as_str()).collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Id;

    #[test]
    fn test_empty_string() {
        assert_eq!(SlimString("000000:".into()), "".to_slim_string());
    }

    #[test]
    fn test_hello_world() {
        assert_eq!(
            SlimString("000011:hello world".into()),
            "hello world".to_slim_string()
        );
    }

    #[test]
    fn test_string() {
        assert_eq!(
            SlimString("000011:hello world".into()),
            String::from("hello world").to_slim_string()
        );
    }

    #[test]
    fn test_empty_array() {
        let array: [String; 0] = [];
        assert_eq!(
            SlimString("000009:[000000:]".into()),
            array.to_slim_string()
        );
    }

    #[test]
    fn test_array() {
        assert_eq!(
            SlimString("000031:[000002:000003:one:000003:two:]".into()),
            ["one", "two"].to_slim_string()
        );
    }

    #[test]
    fn test_vec_and_slice() {
        let vec = vec!["one", "two"];
        assert_eq!(
            SlimString("000031:[000002:000003:one:000003:two:]".into()),
            vec.as_slice().to_slim_string()
        );
        assert_eq!(
            SlimString("000031:[000002:000003:one:000003:two:]".into()),
            vec.to_slim_string()
        );
    }

    #[test]
    fn test_instructions() {
        let id = Id::from("01HFM0NQM3ZS6BBX0ZH6VA6DJX");
        assert_eq!(
            SlimString("000865:[000008:000074:[000003:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000006:import:000009:some_path:]:000084:[000004:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:make:000008:instance:000005:Class:]:000108:[000006:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:make:000008:instance:000005:Class:000004:Arg1:000004:Arg2:]:000087:[000004:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:call:000008:instance:000008:function:]:000111:[000006:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000004:call:000008:instance:000008:function:000004:Arg1:000004:Arg2:]:000084:[000004:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000006:assign:000006:Symbol:000005:value:]:000110:[000005:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000013:callAndAssign:000006:symbol:000008:instance:000008:function:]:000134:[000007:000026:01HFM0NQM3ZS6BBX0ZH6VA6DJX:000013:callAndAssign:000006:symbol:000008:instance:000008:function:000004:Arg1:000004:Arg2:]:]".into()),
            [
                Instruction::Import {
                    id: id.clone(),
                    path: "some_path".into()
                },
                Instruction::Make {
                    id: id.clone(),
                    instance: "instance".into(),
                    class: "Class".into(),
                    args: vec![]
                },
                Instruction::Make {
                    id: id.clone(),
                    instance: "instance".into(),
                    class: "Class".into(),
                    args: vec!["Arg1".into(), "Arg2".into()]
                },
                Instruction::Call {
                    id: id.clone(),
                    instance: "instance".into(),
                    function: "function".into(),
                    args: vec![]
                },
                Instruction::Call {
                    id: id.clone(),
                    instance: "instance".into(),
                    function: "function".into(),
                    args: vec!["Arg1".into(), "Arg2".into()]
                },
                Instruction::Assign {
                    id: id.clone(),
                    symbol: "Symbol".into(),
                    value: "value".into()
                },
                Instruction::CallAndAssign {
                    id: id.clone(),
                    symbol: "symbol".into(),
                    instance: "instance".into(),
                    function: "function".into(),
                    args: vec![]
                },
                Instruction::CallAndAssign {
                    id: id.clone(),
                    symbol: "symbol".into(),
                    instance: "instance".into(),
                    function: "function".into(),
                    args: vec!["Arg1".into(), "Arg2".into()]
                }
            ]
            .to_slim_string()
        );
    }
}
