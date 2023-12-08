use crate::{ClassPath, Constructor, SlimFixture};
use convert_case::{Case, Casing};
use slim_protocol::{
    ByeOrSlimInstructions, ExceptionMessage, FromSlimReader, FromSlimReaderError, Instruction,
    InstructionResult, ToSlimString,
};
use std::{
    collections::HashMap,
    io::{BufReader, Read, Write},
};
use thiserror::Error;

/// Error that can happen while executing an SlimServer
#[derive(Debug, Error)]
pub enum SlimServerError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    FromSlimReaderError(#[from] FromSlimReaderError),
}

pub type SlimClosureConstructor = Box<dyn Fn(Vec<String>) -> Box<dyn SlimFixture>>;

/// The SlimServer responsible to get the Slim commands and execute against the Fixtures.
pub struct SlimServer<R: Read, W: Write> {
    fixtures: HashMap<String, SlimClosureConstructor>,
    instances: HashMap<String, Box<dyn SlimFixture>>,
    libraries: HashMap<String, Box<dyn SlimFixture>>,
    symbols: HashMap<String, String>,
    imports: Vec<String>,
    reader: BufReader<R>,
    writer: W,
}

impl<R: Read, W: Write> SlimServer<R, W> {
    /// Create a new SlimServer
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            fixtures: HashMap::new(),
            instances: HashMap::new(),
            libraries: HashMap::new(),
            symbols: HashMap::new(),
            reader: BufReader::new(reader),
            imports: Vec::new(),
            writer,
        }
    }

    /// Add a new fixture
    pub fn add_fixture<T: ClassPath + Constructor + SlimFixture + 'static>(&mut self) {
        self.fixtures.insert(
            T::class_path(),
            Box::new(|args: Vec<String>| Box::new(T::construct(args)) as Box<dyn SlimFixture>)
                as Box<dyn Fn(Vec<String>) -> Box<dyn SlimFixture>>,
        );
    }

    /// Run the server
    pub fn run(mut self) -> Result<(), SlimServerError> {
        self.writer.write_all(b"Slim -- V0.5\n")?;
        loop {
            match ByeOrSlimInstructions::from_reader(&mut self.reader)? {
                ByeOrSlimInstructions::Bye => break,
                ByeOrSlimInstructions::Instructions(instructions) => {
                    let result = self.execute_instructions(instructions);
                    self.writer.write_all(result.to_slim_string().as_bytes())?;
                }
            }
        }
        Ok(())
    }

    fn execute_instructions(&mut self, instructions: Vec<Instruction>) -> Vec<InstructionResult> {
        let mut results = Vec::new();
        for instruction in instructions {
            match instruction {
                Instruction::Import { id, path } => {
                    self.imports.push(path);
                    results.push(InstructionResult::Ok { id })
                }
                Instruction::Make {
                    id,
                    instance,
                    class,
                    args,
                } => {
                    let class = self.parse_symbol(class);
                    let Some(fixture) = self.find_fixture(&class) else {
                        results.push(InstructionResult::Exception {
                            id,
                            message: ExceptionMessage::new(format!("NO CLASS: {class}")),
                        });
                        continue;
                    };
                    let args = self.parse_symbols(args);
                    if instance.starts_with("library") {
                        self.libraries.insert(instance, fixture(args));
                    } else {
                        self.instances.insert(instance, fixture(args));
                    }
                    results.push(InstructionResult::Ok { id })
                }
                Instruction::Call {
                    id,
                    instance,
                    function,
                    args,
                } => {
                    let args = self.parse_symbols(args);
                    let instances = if instance.starts_with("library") {
                        &mut self.libraries
                    } else {
                        &mut self.instances
                    };
                    let Some(instance) = instances.get_mut(&instance) else {
                        results.push(InstructionResult::Exception {
                            id,
                            message: ExceptionMessage::new(format!("NO_INSTANCE: {instance}")),
                        });
                        continue;
                    };
                    let function = function.to_case(Case::Snake);

                    match instance.execute_method(&function, args) {
                        Ok(value) if value == "/__VOID__/" => {
                            results.push(InstructionResult::Void { id })
                        }
                        Ok(value) => results.push(InstructionResult::String { id, value }),
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
                    let instances = if instance.starts_with("library") {
                        &mut self.libraries
                    } else {
                        &mut self.instances
                    };
                    let Some(instance) = instances.get_mut(&instance) else {
                        results.push(InstructionResult::Exception {
                            id,
                            message: ExceptionMessage::new(format!("NO_INSTANCE: {instance}")),
                        });
                        continue;
                    };
                    let function = function.to_case(Case::Snake);
                    let symbol = symbol.strip_prefix('$').unwrap_or(&symbol).into();
                    match instance.execute_method(&function, args) {
                        Ok(value) if value == "/__VOID__/" => {
                            results.push(InstructionResult::Void { id });
                            self.symbols.insert(symbol, "".into());
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
                    let symbol = symbol.strip_prefix('$').unwrap_or(&symbol).into();
                    self.symbols.insert(symbol, value);
                    results.push(InstructionResult::Ok { id })
                }
            }
        }
        results
    }

    fn find_fixture(&self, class: &str) -> Option<&SlimClosureConstructor> {
        if let Some(fixture) = self.fixtures.get(class) {
            return Some(fixture);
        }
        for class_path in self.imports.iter() {
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

    fn parse_symbol(&self, mut value: String) -> String {
        while let Some((before, after)) = value.split_once('$') {
            if let Some((name, rest)) = after.split_once(' ') {
                let mut new_value = String::from(before);
                new_value += self.symbols.get(name).unwrap_or(&String::new());
                new_value += " ";
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

#[cfg(test)]
mod tests {
    use slim_protocol::Id;
    use std::error::Error;
    use std::io::Cursor;

    use super::*;

    #[test]
    fn execute_import() -> Result<(), Box<dyn Error>> {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let mut slim_server = SlimServer::new(reader, writer);
        let result = slim_server.execute_instructions(vec![
            Instruction::Import {
                id: Id::from("id_1"),
                path: "ExamplePath1".into(),
            },
            Instruction::Import {
                id: Id::from("id_2"),
                path: "ExamplePath2".into(),
            },
        ]);

        assert_eq!(
            vec!["ExamplePath1".to_string(), "ExamplePath2".to_string()],
            slim_server.imports
        );
        assert_eq!(
            vec![
                InstructionResult::Ok {
                    id: Id::from("id_1")
                },
                InstructionResult::Ok {
                    id: Id::from("id_2")
                }
            ],
            result
        );
        Ok(())
    }

    #[test]
    fn execute_make() -> Result<(), Box<dyn Error>> {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let mut slim_server = SlimServer::new(reader, writer);
        slim_server.add_fixture::<TestFixture>();
        let result = slim_server.execute_instructions(vec![
            Instruction::Make {
                id: Id::from("m_1"),
                instance: "Instance1".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
            Instruction::Make {
                id: Id::from("m_2"),
                instance: "Instance2".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
            Instruction::Make {
                id: Id::from("m_3"),
                instance: "libraryInstance".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
        ]);

        assert_eq!(2, slim_server.instances.len());
        assert!(slim_server.instances.contains_key("Instance1"));
        assert!(slim_server.instances.contains_key("Instance2"));
        assert_eq!(1, slim_server.libraries.len());
        assert!(slim_server.libraries.contains_key("libraryInstance"));
        assert_eq!(
            vec![
                InstructionResult::Ok {
                    id: Id::from("m_1")
                },
                InstructionResult::Ok {
                    id: Id::from("m_2")
                },
                InstructionResult::Ok {
                    id: Id::from("m_3")
                },
            ],
            result
        );
        Ok(())
    }

    #[test]
    fn execute_call() -> Result<(), Box<dyn Error>> {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let mut slim_server = SlimServer::new(reader, writer);
        slim_server.add_fixture::<TestFixture>();
        let result = slim_server.execute_instructions(vec![
            Instruction::Make {
                id: Id::from("m_1"),
                instance: "Instance".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
            Instruction::Make {
                id: Id::from("m_2"),
                instance: "libraryInstance".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
            Instruction::Call {
                id: Id::from("c_1"),
                instance: "Instance".into(),
                function: "echo".into(),
                args: vec!["Arg".into()],
            },
            Instruction::Call {
                id: Id::from("c_2"),
                instance: "libraryInstance".into(),
                function: "echo".into(),
                args: vec!["Arg1".into(), "Arg2".into()],
            },
        ]);

        assert_eq!(
            vec![
                InstructionResult::Ok {
                    id: Id::from("m_1")
                },
                InstructionResult::Ok {
                    id: Id::from("m_2")
                },
                InstructionResult::String {
                    id: Id::from("c_1"),
                    value: "Arg".into()
                },
                InstructionResult::String {
                    id: Id::from("c_2"),
                    value: "Arg1,Arg2".into()
                },
            ],
            result
        );
        Ok(())
    }

    #[test]
    fn execute_call_and_assign() -> Result<(), Box<dyn Error>> {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let mut slim_server = SlimServer::new(reader, writer);
        slim_server.add_fixture::<TestFixture>();
        let result = slim_server.execute_instructions(vec![
            Instruction::Make {
                id: Id::from("m_1"),
                instance: "Instance".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
            Instruction::Make {
                id: Id::from("m_2"),
                instance: "libraryInstance".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
            Instruction::CallAndAssign {
                id: Id::from("ca_1"),
                symbol: "$Symbol1".into(),
                instance: "Instance".into(),
                function: "echo".into(),
                args: vec!["Arg".into()],
            },
            Instruction::Call {
                id: Id::from("c_1"),
                instance: "Instance".into(),
                function: "echo".into(),
                args: vec!["$Symbol1 in symbol".into()],
            },
            Instruction::CallAndAssign {
                id: Id::from("ca_2"),
                symbol: "$Symbol2".into(),
                instance: "libraryInstance".into(),
                function: "echo".into(),
                args: vec!["LibraryArg".into()],
            },
            Instruction::Call {
                id: Id::from("c_2"),
                instance: "libraryInstance".into(),
                function: "echo".into(),
                args: vec!["$Symbol2".into(), "Arg2".into()],
            },
        ]);

        assert_eq!(
            vec![
                InstructionResult::Ok {
                    id: Id::from("m_1")
                },
                InstructionResult::Ok {
                    id: Id::from("m_2")
                },
                InstructionResult::String {
                    id: Id::from("ca_1"),
                    value: "Arg".into()
                },
                InstructionResult::String {
                    id: Id::from("c_1"),
                    value: "Arg in symbol".into()
                },
                InstructionResult::String {
                    id: Id::from("ca_2"),
                    value: "LibraryArg".into()
                },
                InstructionResult::String {
                    id: Id::from("c_2"),
                    value: "LibraryArg,Arg2".into()
                },
            ],
            result
        );
        Ok(())
    }

    #[test]
    fn execute_assign() -> Result<(), Box<dyn Error>> {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let mut slim_server = SlimServer::new(reader, writer);
        slim_server.add_fixture::<TestFixture>();
        let result = slim_server.execute_instructions(vec![
            Instruction::Make {
                id: Id::from("m_1"),
                instance: "Instance".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
            Instruction::Assign {
                id: Id::from("a_1"),
                symbol: "$Symbol".into(),
                value: "Value".into(),
            },
            Instruction::Call {
                id: Id::from("c_1"),
                instance: "Instance".into(),
                function: "echo".into(),
                args: vec!["$Symbol in symbol".into()],
            },
        ]);

        assert_eq!(
            vec![
                InstructionResult::Ok {
                    id: Id::from("m_1")
                },
                InstructionResult::Ok {
                    id: Id::from("a_1")
                },
                InstructionResult::String {
                    id: Id::from("c_1"),
                    value: "Value in symbol".into()
                },
            ],
            result
        );
        Ok(())
    }

    #[test]
    fn integration() -> Result<(), Box<dyn Error>> {
        let mut vec =
            Vec::from(b"000068:[000001:000051:[000003:000004:id_1:000006:import:000008:TestPath:]:]000003:bye".as_slice());
        let reader: Cursor<&mut Vec<u8>> = Cursor::new(&mut vec);
        let mut output = Vec::new();
        let writer = Cursor::new(&mut output);
        let slim_server = SlimServer::new(reader, writer);
        slim_server.run()?;
        assert_eq!(
            "Slim -- V0.5\n000048:[000001:000031:[000002:000004:id_1:000002:OK:]:]",
            String::from_utf8_lossy(&output)
        );
        Ok(())
    }

    #[test]
    fn parse_symbol_with_no_symbol() -> Result<(), Box<dyn Error>> {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let slim_server = SlimServer::new(reader, writer);
        assert_eq!(
            "No symbol",
            slim_server.parse_symbol(String::from("No symbol"))
        );
        assert_eq!("", slim_server.parse_symbol(String::from("")));
        Ok(())
    }

    #[test]
    fn parse_symbol_with_symbol_not_in_symbol_list() -> Result<(), Box<dyn Error>> {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let slim_server = SlimServer::new(reader, writer);
        assert_eq!("", slim_server.parse_symbol(String::from("$symbol")));
        assert_eq!(
            "Test ",
            slim_server.parse_symbol(String::from("Test $symbol"))
        );
        Ok(())
    }

    #[test]
    fn parse_symbol() -> Result<(), Box<dyn Error>> {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let mut slim_server = SlimServer::new(reader, writer);
        slim_server
            .symbols
            .insert("symbol".into(), "Symbol Value".into());
        slim_server
            .symbols
            .insert("symbol2".into(), "Symbol Value 2".into());
        assert_eq!(
            "Symbol Value",
            slim_server.parse_symbol(String::from("$symbol"))
        );
        assert_eq!(
            "Test Symbol Value",
            slim_server.parse_symbol(String::from("Test $symbol"))
        );
        assert_eq!(
            "Test Symbol Value and another Symbol Value 2",
            slim_server.parse_symbol(String::from("Test $symbol and another $symbol2"))
        );
        Ok(())
    }

    #[test]
    fn find_fixture_should_prioritize_an_exact_match() {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let mut slim_server = SlimServer::new(reader, writer);
        add_test_fixture_with_path(&mut slim_server, "ExamplePathFixutre", Ok("First".into()));
        add_test_fixture_with_path(
            &mut slim_server,
            "Namespace.ExamplePathFixutre",
            Ok("Second".into()),
        );
        slim_server.imports.push("Namespace".into());

        let mut result = slim_server
            .find_fixture("ExamplePathFixutre".into())
            .unwrap()(Vec::new());
        assert_eq!(
            Ok("First".to_string()),
            result.execute_method("", Vec::new())
        );
    }

    #[test]
    fn find_fixture_should_use_the_imports() {
        let mut vec = Vec::new();
        let reader = Cursor::new(&mut vec);
        let mut vec = Vec::new();
        let writer = Cursor::new(&mut vec);
        let mut slim_server = SlimServer::new(reader, writer);
        add_test_fixture_with_path(
            &mut slim_server,
            "Namespace1.ExamplePathFixutre",
            Ok("First".into()),
        );
        add_test_fixture_with_path(
            &mut slim_server,
            "Namespace2.ExamplePathFixutre",
            Ok("Second".into()),
        );
        slim_server.imports.push("Namespace2".into());
        slim_server.imports.push("Namespace1".into());

        let mut result = slim_server
            .find_fixture("ExamplePathFixutre".into())
            .unwrap()(Vec::new());
        assert_eq!(
            Ok("Second".to_string()),
            result.execute_method("", Vec::new())
        );
    }

    fn add_test_fixture_with_path<R: Read, W: Write>(
        server: &mut SlimServer<R, W>,
        class_path: impl Into<String>,
        return_value: Result<String, crate::ExecuteMethodError>,
    ) {
        server.fixtures.insert(
            class_path.into(),
            Box::new(move |_: Vec<String>| {
                Box::new(TestFixture {
                    return_value: return_value.clone(),
                }) as Box<dyn SlimFixture>
            }) as Box<dyn Fn(Vec<String>) -> Box<dyn SlimFixture>>,
        );
    }

    struct TestFixture {
        return_value: Result<String, crate::ExecuteMethodError>,
    }

    impl SlimFixture for TestFixture {
        fn execute_method(
            &mut self,
            method: &str,
            parms: Vec<String>,
        ) -> Result<String, crate::ExecuteMethodError> {
            match method {
                "echo" => Ok(parms.join(",")),
                _ => self.return_value.clone(),
            }
        }
    }

    impl ClassPath for TestFixture {
        fn class_path() -> String {
            "Test.TestFixture".into()
        }
    }

    impl Constructor for TestFixture {
        fn construct(_: Vec<String>) -> Self {
            Self {
                return_value: Ok("Value".to_string()),
            }
        }
    }
}
