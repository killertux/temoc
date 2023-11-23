use anyhow::{anyhow, Result};
use convert_case::{Case, Casing};
use slim_protocol::{
    ByeOrSlimInstructions, FromSlimReader, Instruction, InstructionResult, ToSlimString,
};
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
pub use rust_slim_macros::*;

pub trait SlimFixture {
    fn execute_method(&mut self, method: &str, args: Vec<String>) -> Result<Option<String>>;
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

pub struct SlimServer<R: Read, W: Write> {
    fixtures: HashMap<String, Box<dyn Fn(Vec<String>) -> Box<dyn SlimFixture>>>,
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
                                let path = path
                                    .split('.')
                                    .map(|part| part.to_case(Case::Snake))
                                    .join("::");
                                self.class_paths.push(path);
                                results.push(InstructionResult::Ok { id })
                            }
                            Instruction::Make {
                                id,
                                instance,
                                class,
                                args,
                            } => match self.find_fixture(class) {
                                Some(fixture) => {
                                    self.instances.insert(instance, fixture(args));
                                    results.push(InstructionResult::Ok { id })
                                }
                                None => results.push(InstructionResult::Exception {
                                    id,
                                    message: "Class not found".into(),
                                    _complete_message: "Class not found".into(),
                                }),
                            },
                            Instruction::Call {
                                id,
                                instance,
                                function,
                                args,
                            } => {
                                let instance = self
                                    .instances
                                    .get_mut(&instance)
                                    .ok_or(anyhow!("Failed loading instance"))?;
                                let function = function.to_case(Case::Snake);

                                match instance.execute_method(&function, args) {
                                    Ok(Some(value)) => {
                                        results.push(InstructionResult::String { id, value })
                                    }
                                    Ok(None) => results.push(InstructionResult::Null { id }),
                                    Err(error) => results.push(InstructionResult::Exception {
                                        id,
                                        message: error.to_string(),
                                        _complete_message: error.to_string(),
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

    fn find_fixture(
        &self,
        class: String,
    ) -> Option<&Box<dyn Fn(Vec<String>) -> Box<dyn SlimFixture>>> {
        let mut class: Vec<String> = class.split('.').map(|s| s.to_string()).collect();
        class
            .last_mut()
            .map(|last| *last = last.to_case(Case::Pascal));
        let class = class.join("::");
        if let Some(fixture) = self.fixtures.get(&class) {
            return Some(fixture);
        }
        for class_path in self.class_paths.iter() {
            let class = format!("{class_path}::{class}");
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
