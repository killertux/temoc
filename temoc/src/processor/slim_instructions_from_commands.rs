use std::fmt::Display;
use ulid::Ulid;

use slim_protocol::InstructionResultValue;
use slim_protocol::{Id, Instruction, SlimValue};

use super::markdown_commands::{
    Class, DecisionTableType, MarkdownCommand, MethodName, Position, Snooze, TableRow, Value,
};

pub type ExpectedResulWithSnooze = (ExpectedResult, Snooze);

pub fn get_instructions_from_commands(
    commands: Vec<MarkdownCommand>,
) -> (Vec<Instruction>, Vec<ExpectedResulWithSnooze>) {
    let mut plan = InstructionPlan::default();
    for command in commands {
        match command {
            MarkdownCommand::Import { path, position } => plan.import(path, position),
            MarkdownCommand::DecisionTable {
                class,
                r#type,
                table,
                snoozed,
                ..
            } => plan.decision_table(class, &r#type, table, &snoozed),
        }
    }
    (plan.instructions, plan.expected_result)
}

/// Instructions sent to the server together, with what each should return.
#[derive(Default)]
pub struct Batch {
    pub instructions: Vec<Instruction>,
    pub expected_result: Vec<ExpectedResulWithSnooze>,
}

/// One text cell of a `#method` table row, compared against one list element.
pub struct MethodCell {
    pub column: usize,
    pub expected: String,
}

pub struct RowPlan {
    pub line: usize,
    pub batch: Batch,
    pub method_cells: Vec<MethodCell>,
}

pub struct TablePlan {
    pub line: usize,
    pub class: String,
    pub columns: Vec<String>,
    pub snoozed: Snooze,
    /// Pending imports, `Make` and `beginTable`.
    pub setup: Batch,
    pub rows: Vec<RowPlan>,
    pub teardown: Batch,
}

pub struct FilePlan {
    pub tables: Vec<TablePlan>,
    /// Imports that come after the last table.
    pub trailing: Batch,
}

/// What surrounds a file when a file hooks fixture is configured. Each part
/// is sent by itself, so that `endFile` still runs after a file that stopped
/// early.
pub struct FileHooks {
    pub make: Batch,
    pub begin: Batch,
    pub end: Batch,
}

/// Builds the calls to the file hooks fixture `class` for `file`. Like the
/// table lifecycle methods, `beginFile` and `endFile` are optional. They
/// belong to no line of the file, so they are reported at line 0.
pub fn file_hooks(class: &str, file: &str) -> FileHooks {
    let position = Position::new(0, 0);
    let instance = Ulid::new().to_string();
    let mut plan = InstructionPlan::default();

    let id = Id::new();
    plan.instructions.push(Instruction::Make {
        id: id.clone(),
        instance: instance.clone(),
        class: class.to_string(),
        args: Vec::new(),
    });
    plan.expected_result.push((
        ExpectedResult::ok(id, position.clone()),
        Snooze::not_snooze(),
    ));
    let make = plan.take_batch();

    let mut hook = |function: &str, args: Vec<SlimValue>| {
        let method_name = MethodName(function.to_string(), position.clone());
        plan.call(
            &instance,
            function.to_string(),
            args,
            &Snooze::not_snooze(),
            |id| {
                ExpectedResult::null_or_void_or_method_not_found(
                    id,
                    position.clone(),
                    Some(method_name),
                )
            },
        );
        plan.take_batch()
    };
    let begin = hook("beginFile", vec![SlimValue::String(file.to_string())]);
    let end = hook("endFile", Vec::new());
    FileHooks { make, begin, end }
}

/// Splits the commands of a file into batches that are sent one at a time.
pub fn get_file_plan(commands: Vec<MarkdownCommand>) -> FilePlan {
    let mut plan = InstructionPlan::default();
    let mut tables = Vec::new();
    for command in commands {
        match command {
            MarkdownCommand::Import { path, position } => plan.import(path, position),
            MarkdownCommand::DecisionTable {
                class,
                r#type,
                table,
                columns,
                snoozed,
            } => tables.push(plan.table_plan(class, &r#type, table, columns, snoozed)),
        }
    }
    FilePlan {
        tables,
        trailing: plan.take_batch(),
    }
}

#[derive(Default)]
struct InstructionPlan {
    instructions: Vec<Instruction>,
    expected_result: Vec<ExpectedResulWithSnooze>,
}

impl InstructionPlan {
    fn import(&mut self, path: String, position: Position) {
        let id = Id::new();
        self.instructions.push(Instruction::Import {
            id: id.clone(),
            path,
        });
        self.expected_result
            .push((ExpectedResult::ok(id, position), Snooze::not_snooze()));
    }

    fn decision_table(
        &mut self,
        Class(test_class, position): Class,
        r#type: &DecisionTableType,
        table: Vec<TableRow>,
        snoozed: &Snooze,
    ) {
        let instance = self.open_table(test_class, position.clone(), snoozed);
        for row in table {
            self.table_row(&instance, r#type, row, snoozed);
        }
        self.optional_call(&instance, "endTable", position, snoozed);
    }

    fn table_plan(
        &mut self,
        Class(test_class, position): Class,
        r#type: &DecisionTableType,
        table: Vec<TableRow>,
        columns: Vec<String>,
        snoozed: Snooze,
    ) -> TablePlan {
        let class = test_class.clone();
        let instance = self.open_table(test_class, position.clone(), &snoozed);
        let setup = self.take_batch();
        let rows = table
            .into_iter()
            .map(|row| {
                let line = row.position.line();
                let method_cells = match r#type {
                    DecisionTableType::MultipleSetterAndGetters => Vec::new(),
                    DecisionTableType::SingleMethod(_) => row
                        .getters
                        .iter()
                        .map(|(_, Value(expected, position))| MethodCell {
                            column: position.column(),
                            expected: expected.clone(),
                        })
                        .collect(),
                };
                self.table_row(&instance, r#type, row, &snoozed);
                RowPlan {
                    line,
                    batch: self.take_batch(),
                    method_cells,
                }
            })
            .collect();
        self.optional_call(&instance, "endTable", position.clone(), &snoozed);
        TablePlan {
            line: position.line(),
            class,
            columns,
            snoozed,
            setup,
            rows,
            teardown: self.take_batch(),
        }
    }

    fn take_batch(&mut self) -> Batch {
        Batch {
            instructions: std::mem::take(&mut self.instructions),
            expected_result: std::mem::take(&mut self.expected_result),
        }
    }

    fn open_table(&mut self, test_class: String, position: Position, snoozed: &Snooze) -> String {
        let instance = Ulid::new().to_string();
        let id = Id::new();
        self.instructions.push(Instruction::Make {
            id: id.clone(),
            instance: instance.clone(),
            class: test_class,
            args: Vec::new(),
        });
        self.expected_result
            .push((ExpectedResult::ok(id, position.clone()), snoozed.clone()));
        self.optional_call(&instance, "beginTable", position, snoozed);
        instance
    }

    fn table_row(
        &mut self,
        instance: &str,
        r#type: &DecisionTableType,
        row: TableRow,
        snoozed: &Snooze,
    ) {
        self.optional_call(instance, "reset", row.position.clone(), snoozed);
        match r#type {
            DecisionTableType::MultipleSetterAndGetters => {
                self.setters_and_getters(instance, row, snoozed);
            }
            DecisionTableType::SingleMethod(method_name) => {
                self.single_method(instance, method_name, row, snoozed);
            }
        }
    }

    fn setters_and_getters(&mut self, instance: &str, row: TableRow, snoozed: &Snooze) {
        for (setter_name, Value(value, position)) in row.setters {
            self.call(
                instance,
                setter_name.0.clone(),
                vec![SlimValue::String(value)],
                snoozed,
                |id| ExpectedResult::null_or_void(id, position, Some(setter_name)),
            );
        }
        self.optional_call(instance, "execute", row.position, snoozed);
        for (getter_name, value) in row.getters {
            self.getter(instance, getter_name, value, snoozed);
        }
    }

    fn getter(
        &mut self,
        instance: &str,
        getter_name: MethodName,
        Value(value, position): Value,
        snoozed: &Snooze,
    ) {
        let function = getter_name.0.clone();
        if let Some(symbol) = value.strip_prefix("$").and_then(|v| v.strip_suffix("=")) {
            let id = Id::new();
            self.instructions.push(Instruction::CallAndAssign {
                id: id.clone(),
                instance: instance.to_string(),
                function,
                symbol: symbol.to_string(),
                args: vec![],
            });
            self.expected_result.push((
                ExpectedResult::set_symbol(id, position, getter_name, symbol.to_string()),
                snoozed.clone(),
            ));
        } else if let Some(symbol) = value.strip_prefix("$") {
            self.call(instance, function, vec![], snoozed, |id| {
                ExpectedResult::symbol(id, position, getter_name, symbol.to_string())
            });
        } else {
            self.call(instance, function, vec![], snoozed, |id| {
                ExpectedResult::string(id, position, getter_name, value)
            });
        }
    }

    fn single_method(
        &mut self,
        instance: &str,
        method_name: &MethodName,
        row: TableRow,
        snoozed: &Snooze,
    ) {
        let params = row
            .setters
            .into_iter()
            .map(|setter| SlimValue::String(setter.1 .0))
            .collect();
        let result = row
            .getters
            .into_iter()
            .map(|getter| ExpectedResultValue::String(getter.1 .0))
            .collect();
        self.call(instance, method_name.0.clone(), params, snoozed, |id| {
            ExpectedResult::list(id, row.position, method_name.clone(), result)
        });
    }

    /// Adds a table lifecycle call, which fixtures are free not to implement.
    fn optional_call(
        &mut self,
        instance: &str,
        function: &str,
        position: Position,
        snoozed: &Snooze,
    ) {
        self.call(instance, function.to_string(), Vec::new(), snoozed, |id| {
            ExpectedResult::null_or_void_or_method_not_found(id, position, None)
        });
    }

    fn call(
        &mut self,
        instance: &str,
        function: String,
        args: Vec<SlimValue>,
        snoozed: &Snooze,
        expected: impl FnOnce(Id) -> ExpectedResult,
    ) {
        let id = Id::new();
        self.instructions.push(Instruction::Call {
            id: id.clone(),
            instance: instance.to_string(),
            function,
            args,
        });
        self.expected_result.push((expected(id), snoozed.clone()));
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct ExpectedResult {
    pub id: Id,
    pub position: Position,
    pub method_name: Option<MethodName>,
    pub value: ExpectedResultValue,
}

impl ExpectedResult {
    pub fn ok(id: Id, position: Position) -> Self {
        Self {
            id,
            position,
            method_name: None,
            value: ExpectedResultValue::Ok,
        }
    }

    #[cfg(test)]
    pub fn any(id: Id, position: Position) -> Self {
        Self {
            id,
            position,
            method_name: None,
            value: ExpectedResultValue::Any,
        }
    }

    pub fn null_or_void(id: Id, position: Position, method_name: Option<MethodName>) -> Self {
        Self {
            id,
            position,
            method_name,
            value: ExpectedResultValue::NullOrVoid,
        }
    }

    pub fn null_or_void_or_method_not_found(
        id: Id,
        position: Position,
        method_name: Option<MethodName>,
    ) -> Self {
        Self {
            id,
            position,
            method_name,
            value: ExpectedResultValue::NullOrVoidOrMethodNotFound,
        }
    }

    pub fn string(id: Id, position: Position, method_name: MethodName, value: String) -> Self {
        Self {
            id,
            position,
            method_name: Some(method_name),
            value: ExpectedResultValue::String(value),
        }
    }

    pub fn set_symbol(id: Id, position: Position, method_name: MethodName, value: String) -> Self {
        Self {
            id,
            position,
            method_name: Some(method_name),
            value: ExpectedResultValue::SetSymbol(value),
        }
    }

    pub fn symbol(id: Id, position: Position, method_name: MethodName, value: String) -> Self {
        Self {
            id,
            position,
            method_name: Some(method_name),
            value: ExpectedResultValue::Symbol(value),
        }
    }

    pub fn list(
        id: Id,
        position: Position,
        method_name: MethodName,
        value: Vec<ExpectedResultValue>,
    ) -> Self {
        Self {
            id,
            position,
            method_name: Some(method_name),
            value: ExpectedResultValue::List(value),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum ExpectedResultValue {
    #[cfg(test)]
    Any,
    Ok,
    NullOrVoid,
    NullOrVoidOrMethodNotFound,
    String(String),
    SetSymbol(String),
    Symbol(String),
    /// A resolved client-side symbol retains the complete recursive wire
    /// value returned by a prior `callAndAssign`.
    Result(InstructionResultValue),
    List(Vec<ExpectedResultValue>),
}

impl Display for ExpectedResultValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(test)]
            ExpectedResultValue::Any => write!(f, "ANY"),
            ExpectedResultValue::NullOrVoid => write!(f, "NULL or VOID"),
            ExpectedResultValue::NullOrVoidOrMethodNotFound => {
                write!(f, "NULL or VOID or NOT FOUND")
            }
            ExpectedResultValue::Ok => write!(f, "OK"),
            ExpectedResultValue::String(value) => write!(f, "`{value}`"),
            ExpectedResultValue::SetSymbol(value) => write!(f, "SET SYMBOL `{value}`"),
            ExpectedResultValue::Symbol(value) => write!(f, "SYMBOL `{value}`"),
            ExpectedResultValue::Result(value) => write!(f, "{value}"),
            ExpectedResultValue::List(value) => {
                write!(
                    f,
                    "[{}]",
                    value
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect::<Vec<String>>()
                        .join(",")
                )
            }
        }
    }
}

#[cfg(test)]
mod test {
    use anyhow::{bail, Result};
    use chrono::NaiveDate;

    use crate::processor::markdown_commands::{DecisionTableType, Snooze, TableRow};

    use super::*;

    #[test]
    fn import() {
        let (instructions, expected_result) =
            get_instructions_from_commands(vec![MarkdownCommand::Import {
                path: "Fixtures".into(),
                position: Position::new(0, 0),
            }]);
        assert_eq!(1, instructions.len());
        assert_eq!(instructions.len(), expected_result.len());
        assert!(
            matches!(&instructions[0], Instruction::Import { id:_, path } if path == "Fixtures"),
        );
        assert!(matches!(
            &expected_result[0].0.value,
            ExpectedResultValue::Ok
        ),);
    }

    #[test]
    fn decision_table() -> Result<()> {
        let position = Position::new(0, 0);
        let (mut instructions, mut expected_result) =
            get_instructions_from_commands(vec![MarkdownCommand::DecisionTable {
                class: Class("Class".into(), position.clone()),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![
                    TableRow {
                        position: Position::new(1, 0),
                        setters: vec![
                            (
                                MethodName("setA".into(), position.clone()),
                                Value("1".into(), position.clone()),
                            ),
                            (
                                MethodName("setB".into(), position.clone()),
                                Value("2".into(), position.clone()),
                            ),
                        ],
                        getters: vec![
                            (
                                MethodName("getA".into(), position.clone()),
                                Value("1".into(), position.clone()),
                            ),
                            (
                                MethodName("getB".into(), position.clone()),
                                Value("2".into(), position.clone()),
                            ),
                        ],
                    },
                    TableRow {
                        position: Position::new(2, 0),
                        setters: vec![
                            (
                                MethodName("setA".into(), position.clone()),
                                Value("3".into(), position.clone()),
                            ),
                            (
                                MethodName("setB".into(), position.clone()),
                                Value("4".into(), position.clone()),
                            ),
                        ],
                        getters: vec![
                            (
                                MethodName("getA".into(), position.clone()),
                                Value("3".into(), position.clone()),
                            ),
                            (
                                MethodName("getB".into(), position.clone()),
                                Value("4".into(), position.clone()),
                            ),
                        ],
                    },
                ],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            }]);
        assert_eq!(15, instructions.len());
        assert_eq!(instructions.len(), expected_result.len());
        let Instruction::Make {
            id,
            instance,
            class,
            args,
        } = instructions.remove(0)
        else {
            bail!("Expected make");
        };
        assert_eq!("Class", class);
        assert_eq!(args, Vec::new());

        assert!(
            matches!(expected_result.remove(0), (ExpectedResult { id: expected_id, position:_, method_name: None, value: ExpectedResultValue::Ok }, snooze) if expected_id == id && !snooze.should_snooze())
        );
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "beginTable" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setA" && args == [SlimValue::String("1".into())]
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id:_, method_name: Some(method_name), position:_, value: ExpectedResultValue::NullOrVoid }, _) if method_name.0 == "setA"
        ));
        assert!(matches!(
            &instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setB" && args == &[SlimValue::String("2".into())]
        ));
        assert!(matches!(
            &expected_result.remove(0),
            (ExpectedResult { id:_, method_name: Some(method_name), position:_, value: ExpectedResultValue::NullOrVoid }, _) if method_name.0 == "setB"
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "execute" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            &instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getA" && args.is_empty()
        ));
        assert!(matches!(
            &expected_result.remove(0),
            (ExpectedResult {
                id: _,
                method_name: Some(method_name),
                value: ExpectedResultValue::String(value),
                position: _
            }, _) if method_name.0 == "getA" && value == "1"
        ));
        assert!(matches!(
            &instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getB" && args.is_empty()
        ));
        assert!(matches!(
            &expected_result.remove(0),

            (ExpectedResult {
                id: _,
                method_name: Some(method_name),
                value: ExpectedResultValue::String(value),
                position: _
            }, _) if method_name.0 == "getB" && value == "2"

        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            &instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setA" && args == &[SlimValue::String("3".into())]
        ));
        assert!(matches!(
            &expected_result.remove(0),
            (ExpectedResult { id:_, method_name: Some(method_name), position:_, value: ExpectedResultValue::NullOrVoid }, _) if method_name.0 == "setA"
        ));
        assert!(matches!(
            &instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setB" && args == &[SlimValue::String("4".into())]
        ));
        assert!(matches!(
            &expected_result.remove(0),
            (ExpectedResult { id:_, method_name: Some(method_name), position:_, value: ExpectedResultValue::NullOrVoid }, _) if method_name.0 == "setB"
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "execute" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            &instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getA" && args.is_empty()
        ));
        assert!(matches!(
            &expected_result.remove(0),
            (ExpectedResult {
                id: _,
                method_name: Some(method_name),
                value: ExpectedResultValue::String(value),
                position: _
            }, _) if method_name.0 == "getA" && value == "3"
        ));
        assert!(matches!(
            &instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getB" && args.is_empty()
        ));
        assert!(matches!(
            &expected_result.remove(0),
            (ExpectedResult {
                id: _,
                method_name: Some(method_name),
                value: ExpectedResultValue::String(value),
                position: _
            }, _) if method_name.0 == "getB" && value == "4"
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "endTable" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        Ok(())
    }

    #[test]
    fn snooze() -> Result<()> {
        let position = Position::new(0, 0);
        let (mut instructions, mut expected_result) =
            get_instructions_from_commands(vec![MarkdownCommand::DecisionTable {
                class: Class("Class".into(), position.clone()),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![TableRow {
                    position: position.clone(),
                    setters: vec![(
                        MethodName("setA".into(), position.clone()),
                        Value("1".into(), position.clone()),
                    )],
                    getters: vec![(
                        MethodName("getA".into(), position.clone()),
                        Value("1".into(), position.clone()),
                    )],
                }],
                columns: vec![],
                snoozed: Snooze::snooze(NaiveDate::from_ymd_opt(2099, 12, 31).unwrap()),
            }]);
        assert_eq!(7, instructions.len());
        assert_eq!(instructions.len(), expected_result.len());
        let Instruction::Make {
            id,
            instance,
            class,
            args,
        } = instructions.remove(0)
        else {
            bail!("Expected make");
        };
        assert_eq!("Class", class);
        assert_eq!(args, Vec::new());
        assert!(
            matches!(expected_result.remove(0), (ExpectedResult { id: expected_id, position:_, method_name: None, value: ExpectedResultValue::Ok }, snooze) if expected_id == id && snooze.should_snooze())
        );
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "beginTable" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id: _, position:_, method_name: None, value: ExpectedResultValue::NullOrVoidOrMethodNotFound }, snooze) if snooze.should_snooze()
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id: _, position:_, method_name: None, value: ExpectedResultValue::NullOrVoidOrMethodNotFound }, snooze) if snooze.should_snooze()
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setA" && args == [SlimValue::String("1".into())]
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id:_, method_name: Some(method_name), position:_, value: ExpectedResultValue::NullOrVoid }, snooze) if method_name.0 == "setA" && snooze.should_snooze()
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "execute" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id: _, position:_, method_name: None, value: ExpectedResultValue::NullOrVoidOrMethodNotFound }, snooze) if snooze.should_snooze()
        ));
        assert!(matches!(
            &instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getA" && args.is_empty()
        ));
        assert!(matches!(
            &expected_result.remove(0),
            (ExpectedResult {
                id: _,
                value: ExpectedResultValue::String(value),
                method_name: Some(method_name),
                position: _
            }, snooze) if method_name.0 == "getA" && value == "1" && snooze.should_snooze()
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "endTable" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id: _, position:_, method_name: None, value: ExpectedResultValue::NullOrVoidOrMethodNotFound }, snooze) if snooze.should_snooze()
        ));
        Ok(())
    }

    #[test]
    fn decision_table_single_method() -> Result<()> {
        let position = Position::new(0, 0);
        let (mut instructions, mut expected_result) =
            get_instructions_from_commands(vec![MarkdownCommand::DecisionTable {
                class: Class("Class".into(), position.clone()),
                r#type: DecisionTableType::SingleMethod(MethodName(
                    "Method".into(),
                    position.clone(),
                )),
                table: vec![
                    TableRow {
                        position: Position::new(1, 0),
                        setters: vec![
                            (
                                MethodName("setA".into(), position.clone()),
                                Value("1".into(), position.clone()),
                            ),
                            (
                                MethodName("setB".into(), position.clone()),
                                Value("2".into(), position.clone()),
                            ),
                        ],
                        getters: vec![
                            (
                                MethodName("getA".into(), position.clone()),
                                Value("1".into(), position.clone()),
                            ),
                            (
                                MethodName("getB".into(), position.clone()),
                                Value("2".into(), position.clone()),
                            ),
                        ],
                    },
                    TableRow {
                        position: Position::new(2, 0),
                        setters: vec![
                            (
                                MethodName("setA".into(), position.clone()),
                                Value("3".into(), position.clone()),
                            ),
                            (
                                MethodName("setB".into(), position.clone()),
                                Value("4".into(), position.clone()),
                            ),
                        ],
                        getters: vec![
                            (
                                MethodName("getA".into(), position.clone()),
                                Value("3".into(), position.clone()),
                            ),
                            (
                                MethodName("getB".into(), position.clone()),
                                Value("4".into(), position.clone()),
                            ),
                        ],
                    },
                ],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            }]);
        assert_eq!(7, instructions.len());
        assert_eq!(instructions.len(), expected_result.len());
        let Instruction::Make {
            id,
            instance,
            class,
            args,
        } = instructions.remove(0)
        else {
            bail!("Expected make");
        };
        assert_eq!("Class", class);
        assert_eq!(args, Vec::new());

        assert!(
            matches!(expected_result.remove(0), (ExpectedResult { id: expected_id, position:_, method_name: None, value: ExpectedResultValue::Ok }, snooze) if expected_id == id && !snooze.should_snooze())
        );
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "beginTable" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "Method" && args == [SlimValue::String("1".into()), SlimValue::String("2".into())]
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id:_, method_name: Some(method_name), position:_, value: ExpectedResultValue::List(list) }, _)
                if method_name.0 == "Method" && list.len() == 2
                    && list[0] == ExpectedResultValue::String("1".into())
                    && list[1] == ExpectedResultValue::String("2".into())
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "Method" && args == [SlimValue::String("3".into()), SlimValue::String("4".into())]
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id:_, method_name: Some(method_name), position:_, value: ExpectedResultValue::List(list) }, _)
                if method_name.0 == "Method" && list.len() == 2
                    && list[0] == ExpectedResultValue::String("3".into())
                    && list[1] == ExpectedResultValue::String("4".into())
        ));

        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "endTable" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        Ok(())
    }

    #[test]
    fn test_symbols() -> Result<()> {
        let position = Position::new(0, 0);
        let (mut instructions, mut expected_result) =
            get_instructions_from_commands(vec![MarkdownCommand::DecisionTable {
                class: Class("Class".into(), position.clone()),
                r#type: DecisionTableType::MultipleSetterAndGetters,
                table: vec![
                    TableRow {
                        position: position.clone(),
                        setters: vec![],
                        getters: vec![(
                            MethodName("getA".into(), position.clone()),
                            Value("$V=".into(), position.clone()),
                        )],
                    },
                    TableRow {
                        position: position.clone(),
                        setters: vec![],
                        getters: vec![(
                            MethodName("getA".into(), position.clone()),
                            Value("$V".into(), position.clone()),
                        )],
                    },
                ],
                columns: vec![],
                snoozed: Snooze::not_snooze(),
            }]);
        assert_eq!(9, instructions.len());
        assert_eq!(instructions.len(), expected_result.len());
        let Instruction::Make {
            id,
            instance,
            class,
            args,
        } = instructions.remove(0)
        else {
            bail!("Expected make");
        };
        assert_eq!("Class", class);
        assert_eq!(args, Vec::new());
        assert!(
            matches!(expected_result.remove(0), (ExpectedResult { id: expected_id, position:_, method_name: None, value: ExpectedResultValue::Ok }, _) if expected_id == id)
        );
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "beginTable" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "execute" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::CallAndAssign {
                id: _,
                instance: expected_instance,
                function,
                symbol,
                args
            } if *expected_instance == instance && function == "getA" && symbol == "V" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id: _, position:_, method_name: Some(method), value: ExpectedResultValue::SetSymbol(symbol) }, _) if symbol == "V" && method.0 == "getA"
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "execute" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getA" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (ExpectedResult { id: _, position:_, method_name: Some(method), value: ExpectedResultValue::Symbol(symbol) }, _) if symbol == "V" && method.0 == "getA"
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "endTable" && args.is_empty()
        ));
        assert!(matches!(
            expected_result.remove(0),
            (
                ExpectedResult {
                    id: _,
                    position: _,
                    method_name: None,
                    value: ExpectedResultValue::NullOrVoidOrMethodNotFound
                },
                _
            )
        ));
        Ok(())
    }
}
