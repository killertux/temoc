use anyhow::Result;
use convert_case::{Case, Casing};
pub use rust_slim_macros::*;
use slim_protocol::{
    ByeOrSlimInstructions, ExceptionMessage, FromSlimReader, Instruction, InstructionResult,
    ToSlimString,
};
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};

#[derive(Debug, PartialEq, Eq)]
pub enum ExecuteMethodError {
    MethodNotFound { method: String, class: String },
    ArgumentParsingError(String),
    ExecutionError(String),
}

impl ToString for ExecuteMethodError {
    fn to_string(&self) -> String {
        match self {
            ExecuteMethodError::MethodNotFound { method, class } => {
                format!("NO_METHOD_IN_CLASS {method} {class}")
            }
            ExecuteMethodError::ArgumentParsingError(argument) => {
                format!("NO_CONVERTER_FOR_ARGUMENT_NUMBER {argument}")
            }
            ExecuteMethodError::ExecutionError(error) => error.to_string(),
        }
    }
}

pub fn from_rust_module_path_to_class_path(rust_module_path: &str) -> String {
    rust_module_path
        .split("::")
        .map(|s| s.to_case(Case::Pascal))
        .join(".")
}

pub trait ToSlimResultString {
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError>;
}

macro_rules! impl_to_slim_result_string {
    ($t:ident) => {
        impl ToSlimResultString for $t {
            fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
                Ok(self.to_string())
            }
        }
    };
}

impl_to_slim_result_string!(u8);
impl_to_slim_result_string!(u16);
impl_to_slim_result_string!(u32);
impl_to_slim_result_string!(u64);
impl_to_slim_result_string!(usize);
impl_to_slim_result_string!(i8);
impl_to_slim_result_string!(i16);
impl_to_slim_result_string!(i32);
impl_to_slim_result_string!(i64);
impl_to_slim_result_string!(isize);
impl_to_slim_result_string!(f32);
impl_to_slim_result_string!(f64);
impl_to_slim_result_string!(String);

impl ToSlimResultString for () {
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        Ok(String::from("null"))
    }
}

impl<'a> ToSlimResultString for &'a str {
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        Ok(self.to_string())
    }
}

impl<T> ToSlimResultString for Option<T>
where
    T: ToString,
{
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        match self {
            None => Ok(String::from("null")),
            Some(value) => Ok(value.to_string()),
        }
    }
}

impl<T, E> ToSlimResultString for Result<T, E>
where
    T: ToString,
    E: ToString,
{
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        match self {
            Err(e) => Err(ExecuteMethodError::ExecutionError(e.to_string())),
            Ok(value) => Ok(value.to_string()),
        }
    }
}

pub trait SlimFixture {
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<String>,
    ) -> Result<String, ExecuteMethodError>;
}

pub trait ClassPath {
    fn class_path() -> String;
}

pub trait Constructor {
    fn construct(args: Vec<String>) -> Self;
}

impl<T> Constructor for T
where
    T: Default,
{
    fn construct(_args: Vec<String>) -> T {
        T::default()
    }
}

pub type SlimClosureConstructor = Box<dyn Fn(Vec<String>) -> Box<dyn SlimFixture>>;
pub struct SlimServer<R: Read, W: Write> {
    fixtures: HashMap<String, SlimClosureConstructor>,
    instances: HashMap<String, Box<dyn SlimFixture>>,
    class_paths: Vec<String>,
    reader: BufReader<R>,
    writer: W,
}

impl<R: Read, W: Write> SlimServer<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            fixtures: HashMap::new(),
            instances: HashMap::new(),
            reader: BufReader::new(reader),
            class_paths: Vec::new(),
            writer,
        }
    }

    pub fn add_fixture<T: ClassPath + Constructor + SlimFixture + 'static>(&mut self) {
        self.fixtures.insert(
            T::class_path(),
            Box::new(|args: Vec<String>| Box::new(T::construct(args)) as Box<dyn SlimFixture>)
                as Box<dyn Fn(Vec<String>) -> Box<dyn SlimFixture>>,
        );
    }

    pub fn run(mut self) -> Result<()> {
        self.writer.write_all(b"Slim -- V0.5\n")?;
        loop {
            match ByeOrSlimInstructions::from_reader(&mut self.reader)? {
                ByeOrSlimInstructions::Bye => break,
                ByeOrSlimInstructions::Instructions(instructions) => {
                    let mut results = Vec::new();
                    for instruction in instructions {
                        match instruction {
                            Instruction::Import { id, path } => {
                                self.class_paths.push(path);
                                results.push(InstructionResult::Ok { id })
                            }
                            Instruction::Make {
                                id,
                                instance,
                                class,
                                args,
                            } => match self.find_fixture(&class) {
                                Some(fixture) => {
                                    self.instances.insert(instance, fixture(args));
                                    results.push(InstructionResult::Ok { id })
                                }
                                None => results.push(InstructionResult::Exception {
                                    id,
                                    message: ExceptionMessage::new(format!("NO CLASS: {class}")),
                                }),
                            },
                            Instruction::Call {
                                id,
                                instance,
                                function,
                                args,
                            } => {
                                let Some(instance) = self.instances.get_mut(&instance) else {
                                    results.push(InstructionResult::Exception {
                                        id,
                                        message: ExceptionMessage::new(format!(
                                            "NO_INSTANCE: {instance}"
                                        )),
                                    });
                                    continue;
                                };
                                let function = function.to_case(Case::Snake);

                                match instance.execute_method(&function, args) {
                                    Ok(value) if value == "null" => {
                                        results.push(InstructionResult::Null { id })
                                    }
                                    Ok(value) => {
                                        results.push(InstructionResult::String { id, value })
                                    }
                                    Err(error) => results.push(InstructionResult::Exception {
                                        id,
                                        message: ExceptionMessage::new(error.to_string()),
                                    }),
                                }
                            }
                            _ => todo!("Not implemented"),
                        }
                    }
                    self.writer.write_all(results.to_slim_string().as_bytes())?;
                }
            }
        }
        Ok(())
    }

    fn find_fixture(&self, class: &str) -> Option<&SlimClosureConstructor> {
        if let Some(fixture) = self.fixtures.get(class) {
            return Some(fixture);
        }
        for class_path in self.class_paths.iter() {
            let class = format!("{class_path}.{class}");
            if let Some(fixture) = self.fixtures.get(&class) {
                return Some(fixture);
            }
        }
        None
    }
}

trait Join<'a> {
    fn join(self, separator: &'a str) -> String;
}

impl<'a, T, R> Join<'a> for T
where
    T: Iterator<Item = R>,
    R: AsRef<str>,
{
    fn join(self, separator: &'a str) -> String {
        let mut result = String::new();
        let mut first = true;
        for part in self {
            if !first {
                result += separator;
            }
            result += part.as_ref();
            first = false
        }
        result
    }
}