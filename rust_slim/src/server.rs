use crate::{ClassPath, Constructor, SlimFixture};
use anyhow::Result;
use convert_case::{Case, Casing};
use slim_protocol::{
    ByeOrSlimInstructions, ExceptionMessage, FromSlimReader, Instruction, InstructionResult,
    ToSlimString,
};
use std::{
    collections::HashMap,
    io::{BufReader, Read, Write},
};

pub type SlimClosureConstructor = Box<dyn Fn(Vec<String>) -> Box<dyn SlimFixture>>;
pub struct SlimServer<R: Read, W: Write> {
    fixtures: HashMap<String, SlimClosureConstructor>,
    instances: HashMap<String, Box<dyn SlimFixture>>,
    libraries: HashMap<String, Box<dyn SlimFixture>>,
    symbols: HashMap<String, String>,
    class_paths: Vec<String>,
    reader: BufReader<R>,
    writer: W,
}

impl<R: Read, W: Write> SlimServer<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            fixtures: HashMap::new(),
            instances: HashMap::new(),
            libraries: HashMap::new(),
            symbols: HashMap::new(),
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
                            } => {
                                let class = self.parse_symbol(class);
                                match self.find_fixture(&class) {
                                    Some(fixture) => {
                                        let args = self.parse_symbols(args);
                                        if instance.starts_with("library") {
                                            self.libraries.insert(instance, fixture(args));
                                        } else {
                                            self.instances.insert(instance, fixture(args));
                                        }

                                        results.push(InstructionResult::Ok { id })
                                    }
                                    None => results.push(InstructionResult::Exception {
                                        id,
                                        message: ExceptionMessage::new(format!(
                                            "NO CLASS: {class}"
                                        )),
                                    }),
                                }
                            }
                            Instruction::Call {
                                id,
                                instance,
                                function,
                                args,
                            } => {
                                let args = self.parse_symbols(args);
                                let Some(instance) = if instance.starts_with("library") {
                                    &mut self.instances
                                } else {
                                    &mut self.libraries
                                }
                                .get_mut(&instance) else {
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
                            Instruction::CallAndAssign {
                                id,
                                symbol,
                                instance,
                                function,
                                args,
                            } => {
                                let args = self.parse_symbols(args);
                                let Some(instance) = if instance.starts_with("library") {
                                    &mut self.instances
                                } else {
                                    &mut self.libraries
                                }
                                .get_mut(&instance) else {
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
                                        results.push(InstructionResult::Null { id });
                                        self.symbols.insert(symbol, value);
                                    }
                                    Ok(value) => {
                                        results.push(InstructionResult::String {
                                            id,
                                            value: value.clone(),
                                        });
                                        self.symbols.insert(symbol, value);
                                    }
                                    Err(error) => results.push(InstructionResult::Exception {
                                        id,
                                        message: ExceptionMessage::new(error.to_string()),
                                    }),
                                }
                            }
                            Instruction::Assign { id, symbol, value } => {
                                self.symbols.insert(symbol, value);
                                results.push(InstructionResult::Ok { id })
                            }
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

    fn parse_symbols(&self, args: Vec<String>) -> Vec<String> {
        args.into_iter().map(|arg| self.parse_symbol(arg)).collect()
    }

    fn parse_symbol<'a>(&self, mut value: String) -> String {
        while let Some((before, after)) = value.split_once('$') {
            if let Some((name, rest)) = after.split_once(' ') {
                let mut new_value = String::from(before);
                new_value += self.symbols.get(name).unwrap_or(&String::new());
                new_value += rest;
                value = new_value;
            } else {
                let mut new_value = String::from(before);
                new_value += self.symbols.get(after).unwrap_or(&String::new());
                value = new_value;
            }
        }
        value
    }
}
