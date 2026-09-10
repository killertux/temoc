use crate::{ClassPath, Constructor, SlimFixture, SlimValue};
use convert_case::{Case, Casing};
use slim_protocol::{
    ByeOrSlimInstructions, ExceptionMessage, FromSlimReader, FromSlimReaderError, Instruction,
    InstructionResult, InstructionResultValue, SlimValue as WireSlimValue, ToSlimString,
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

pub type SlimClosureConstructor = Box<dyn Fn(Vec<SlimValue>) -> Box<dyn SlimFixture>>;

/// The SlimServer responsible to get the Slim commands and execute against the Fixtures.
pub struct SlimServer<R: Read, W: Write> {
    fixtures: HashMap<String, SlimClosureConstructor>,
    instances: HashMap<String, Box<dyn SlimFixture>>,
    libraries: HashMap<String, Box<dyn SlimFixture>>,
    symbols: HashMap<String, SlimValue>,
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
            Box::new(|args: Vec<SlimValue>| Box::new(T::construct(args)) as Box<dyn SlimFixture>)
                as Box<dyn Fn(Vec<SlimValue>) -> Box<dyn SlimFixture>>,
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
                Instruction::Malformed { id, fields } => {
                    results.push(InstructionResult::exception(
                        id,
                        ExceptionMessage::new(format!(
                            "MALFORMED_INSTRUCTION [{}]",
                            fields.join(",")
                        )),
                    ));
                }
                Instruction::Import { id, path } => {
                    self.imports.push(path);
                    results.push(InstructionResult::ok(id))
                }
                Instruction::Make {
                    id,
                    instance,
                    class,
                    args,
                } => {
                    let class = self.parse_symbol(class);
                    let Some(fixture) = self.find_fixture(&class) else {
                        results.push(InstructionResult::exception(
                            id,
                            ExceptionMessage::new(format!("NO CLASS: {class}")),
                        ));
                        continue;
                    };
                    let args = self.parse_wire_values(args);
                    if instance.starts_with("library") {
                        self.libraries.insert(instance, fixture(args));
                    } else {
                        self.instances.insert(instance, fixture(args));
                    }
                    results.push(InstructionResult::ok(id))
                }
                Instruction::Call {
                    id,
                    instance,
                    function,
                    args,
                } => {
                    let args = self.parse_wire_values(args);
                    let instances = if instance.starts_with("library") {
                        &mut self.libraries
                    } else {
                        &mut self.instances
                    };
                    let Some(instance) = instances.get_mut(&instance) else {
                        results.push(InstructionResult::exception(
                            id,
                            ExceptionMessage::new(format!("NO_INSTANCE: {instance}")),
                        ));
                        continue;
                    };
                    let function = function.to_case(Case::Snake);

                    match instance.execute_method(&function, args) {
                        Ok(value) => results.push(instruction_result_for_value(id, value)),
                        Err(error) => results.push(InstructionResult::exception(
                            id,
                            ExceptionMessage::new(error.to_string()),
                        )),
                    }
                }
                Instruction::CallAndAssign {
                    id,
                    symbol,
                    instance,
                    function,
                    args,
                } => {
                    let args = self.parse_wire_values(args);
                    let instances = if instance.starts_with("library") {
                        &mut self.libraries
                    } else {
                        &mut self.instances
                    };
                    let Some(instance) = instances.get_mut(&instance) else {
                        results.push(InstructionResult::exception(
                            id,
                            ExceptionMessage::new(format!("NO_INSTANCE: {instance}")),
                        ));
                        continue;
                    };
                    let function = function.to_case(Case::Snake);
                    let symbol = symbol.strip_prefix('$').unwrap_or(&symbol).into();
                    match instance.execute_method(&function, args) {
                        Ok(value) => {
                            results.push(instruction_result_for_value(id, value.clone()));
                            self.symbols.insert(symbol, value);
                        }
                        Err(error) => results.push(InstructionResult::exception(
                            id,
                            ExceptionMessage::new(error.to_string()),
                        )),
                    }
                }
                Instruction::Assign { id, symbol, value } => {
                    let symbol = symbol.strip_prefix('$').unwrap_or(&symbol).into();
                    let value = self.parse_wire_value(value);
                    self.symbols.insert(symbol, value);
                    results.push(InstructionResult::ok(id))
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

    fn parse_wire_values(&self, args: Vec<WireSlimValue>) -> Vec<SlimValue> {
        args.into_iter()
            .map(|arg| self.parse_wire_value(arg))
            .collect()
    }

    fn parse_wire_value(&self, value: WireSlimValue) -> SlimValue {
        match value {
            WireSlimValue::String(value) if value == "null" => SlimValue::Null,
            WireSlimValue::String(value) => SlimValue::String(self.parse_symbol(value)),
            WireSlimValue::List(values) => SlimValue::List(
                values
                    .into_iter()
                    .map(|value| self.parse_wire_value(value))
                    .collect(),
            ),
        }
    }

    fn parse_symbol(&self, mut value: String) -> String {
        while let Some((before, after)) = value.split_once('$') {
            if let Some((name, rest)) = after.split_once(' ') {
                let mut new_value = String::from(before);
                new_value += &self
                    .symbols
                    .get(name)
                    .map(SlimValue::as_text)
                    .unwrap_or_default();
                new_value += " ";
                new_value += rest;
                value = new_value;
            } else {
                let mut new_value = String::from(before);
                new_value += &self
                    .symbols
                    .get(after)
                    .map(SlimValue::as_text)
                    .unwrap_or_default();
                value = new_value;
            }
        }
        value
    }
}

fn instruction_result_for_value(id: slim_protocol::Id, value: SlimValue) -> InstructionResult {
    match value {
        SlimValue::Void => InstructionResult::void(id),
        value => InstructionResult::new(id, instruction_result_value(value)),
    }
}

fn instruction_result_value(value: SlimValue) -> InstructionResultValue {
    match value {
        SlimValue::String(value) => InstructionResultValue::String(value),
        SlimValue::List(values) => {
            InstructionResultValue::List(values.into_iter().map(instruction_result_value).collect())
        }
        SlimValue::Null => InstructionResultValue::String("null".into()),
        SlimValue::Void => InstructionResultValue::Void,
        SlimValue::Object(value) => InstructionResultValue::String(value.display_value().into()),
    }
}

#[cfg(test)]
mod tests {
    use slim_protocol::{FromSlimReader, Id, ToSlimString};
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
                InstructionResult::ok(Id::from("id_1")),
                InstructionResult::ok(Id::from("id_2"))
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
                InstructionResult::ok(Id::from("m_1")),
                InstructionResult::ok(Id::from("m_2")),
                InstructionResult::ok(Id::from("m_3")),
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
                InstructionResult::ok(Id::from("m_1")),
                InstructionResult::ok(Id::from("m_2")),
                InstructionResult::string(Id::from("c_1"), "Arg".into()),
                InstructionResult::string(Id::from("c_2"), "Arg1,Arg2".into()),
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
                InstructionResult::ok(Id::from("m_1")),
                InstructionResult::ok(Id::from("m_2")),
                InstructionResult::string(Id::from("ca_1"), "Arg".into()),
                InstructionResult::string(Id::from("c_1"), "Arg in symbol".into()),
                InstructionResult::string(Id::from("ca_2"), "LibraryArg".into()),
                InstructionResult::string(Id::from("c_2"), "LibraryArg,Arg2".into()),
            ],
            result
        );
        Ok(())
    }

    #[test]
    fn calls_preserve_nested_lists_and_call_and_assign_keeps_the_typed_value(
    ) -> Result<(), Box<dyn Error>> {
        let reader = Cursor::new(Vec::new());
        let writer = Cursor::new(Vec::new());
        let mut slim_server = SlimServer::new(reader, writer);
        slim_server.add_fixture::<TestFixture>();
        let nested = WireSlimValue::List(vec![
            WireSlimValue::String("one".into()),
            WireSlimValue::List(vec![WireSlimValue::String("two".into())]),
        ]);
        let result = slim_server.execute_instructions(vec![
            Instruction::Make {
                id: Id::from("make"),
                instance: "fixture".into(),
                class: "Test.TestFixture".into(),
                args: Vec::new(),
            },
            Instruction::CallAndAssign {
                id: Id::from("call"),
                symbol: "$nested".into(),
                instance: "fixture".into(),
                function: "nested".into(),
                args: vec![nested],
            },
            Instruction::Call {
                id: Id::from("embedded"),
                instance: "fixture".into(),
                function: "echo".into(),
                args: vec!["prefix $nested suffix".into()],
            },
        ]);

        assert_eq!(
            InstructionResult::list(
                Id::from("call"),
                vec![InstructionResultValue::List(vec![
                    InstructionResultValue::String("one".into()),
                    InstructionResultValue::List(vec![InstructionResultValue::String(
                        "two".into()
                    )]),
                ])],
            ),
            result[1]
        );
        assert!(matches!(
            slim_server.symbols.get("nested"),
            Some(SlimValue::List(_))
        ));
        assert_eq!(
            InstructionResult::string(Id::from("embedded"), "prefix [[one, [two]]] suffix".into()),
            result[2]
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
                InstructionResult::ok(Id::from("m_1")),
                InstructionResult::ok(Id::from("a_1")),
                InstructionResult::string(Id::from("c_1"), "Value in symbol".into()),
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
    fn malformed_instruction_returns_an_exception_and_keeps_the_batch_running(
    ) -> Result<(), Box<dyn Error>> {
        let mut input = vec![
            vec!["bad_id", "unknown", "field"],
            vec!["good_id", "import", "TestPath"],
        ]
        .to_slim_string()
        .as_bytes()
        .to_vec();
        input.extend_from_slice("bye".to_slim_string().as_bytes());

        let mut output = Vec::new();
        SlimServer::new(Cursor::new(input), Cursor::new(&mut output)).run()?;

        let mut response = Cursor::new(&output[b"Slim -- V0.5\n".len()..]);
        assert_eq!(
            vec![
                InstructionResult::exception(
                    Id::from("bad_id"),
                    ExceptionMessage::new("MALFORMED_INSTRUCTION [bad_id,unknown,field]".into()),
                ),
                InstructionResult::ok(Id::from("good_id")),
            ],
            Vec::<InstructionResult>::from_reader(&mut response)?
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

        let mut result = slim_server.find_fixture("ExamplePathFixutre").unwrap()(Vec::new());
        assert_eq!(
            Ok(SlimValue::String("First".to_string())),
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

        let mut result = slim_server.find_fixture("ExamplePathFixutre").unwrap()(Vec::new());
        assert_eq!(
            Ok(SlimValue::String("Second".to_string())),
            result.execute_method("", Vec::new())
        );
    }

    fn add_test_fixture_with_path<R: Read, W: Write>(
        server: &mut SlimServer<R, W>,
        class_path: impl Into<String>,
        return_value: Result<SlimValue, crate::ExecuteMethodError>,
    ) {
        server.fixtures.insert(
            class_path.into(),
            Box::new(move |_: Vec<SlimValue>| {
                Box::new(TestFixture {
                    return_value: return_value.clone(),
                }) as Box<dyn SlimFixture>
            }) as Box<dyn Fn(Vec<SlimValue>) -> Box<dyn SlimFixture>>,
        );
    }

    struct TestFixture {
        return_value: Result<SlimValue, crate::ExecuteMethodError>,
    }

    impl SlimFixture for TestFixture {
        fn execute_method(
            &mut self,
            method: &str,
            parms: Vec<SlimValue>,
        ) -> Result<SlimValue, crate::ExecuteMethodError> {
            match method {
                "echo" => Ok(SlimValue::String(
                    parms
                        .into_iter()
                        .map(|value| value.as_text())
                        .collect::<Vec<_>>()
                        .join(","),
                )),
                "nested" => Ok(SlimValue::List(parms)),
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
        fn construct(_: Vec<SlimValue>) -> Self {
            Self {
                return_value: Ok(SlimValue::String("Value".to_string())),
            }
        }
    }
}
