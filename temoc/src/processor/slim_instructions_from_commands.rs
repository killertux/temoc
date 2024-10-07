use anyhow::Result;
use std::fmt::Display;
use ulid::Ulid;

use slim_protocol::{Id, Instruction};

use super::markdown_commands::{
    Class, DecisionTableType, MarkdownCommand, MethodName, Position, Snooze, Value,
};

pub type ExpectedResulWithSnooze = (ExpectedResult, Snooze);

pub fn get_instructions_from_commands(
    commands: Vec<MarkdownCommand>,
) -> Result<(Vec<Instruction>, Vec<ExpectedResulWithSnooze>)> {
    let mut instructions = Vec::new();
    let mut expected_result = Vec::new();
    for command in commands {
        match command {
            MarkdownCommand::Import { path, position } => {
                let id = Id::new();
                instructions.push(Instruction::Import {
                    id: id.clone(),
                    path,
                });
                expected_result.push((ExpectedResult::ok(id, position), Snooze::not_snooze()))
            }
            MarkdownCommand::DecisionTable {
                class: Class(test_class, position),
                r#type,
                table,
                snoozed,
            } => {
                let table_instance = Ulid::new().to_string();
                let id = Id::new();
                instructions.push(Instruction::Make {
                    id: id.clone(),
                    instance: table_instance.clone(),
                    class: test_class,
                    args: Vec::new(),
                });
                expected_result.push((ExpectedResult::ok(id, position.clone()), snoozed.clone()));
                let id = Id::new();
                instructions.push(Instruction::Call {
                    id: id.clone(),
                    instance: table_instance.clone(),
                    function: "beginTable".into(),
                    args: Vec::new(),
                });
                expected_result.push((
                    ExpectedResult::null_or_void(id, position.clone(), None),
                    snoozed.clone(),
                ));
                for row in table.into_iter() {
                    let id = Id::new();
                    instructions.push(Instruction::Call {
                        id: id.clone(),
                        instance: table_instance.clone(),
                        function: "reset".into(),
                        args: Vec::new(),
                    });
                    expected_result.push((
                        ExpectedResult::null_or_void(id, row.position.clone(), None),
                        snoozed.clone(),
                    ));
                    match r#type {
                        DecisionTableType::MultipleSetterAndGetters => {
                            for (setter_name, Value(value, position)) in row.setters.into_iter() {
                                let id = Id::new();
                                instructions.push(Instruction::Call {
                                    id: id.clone(),
                                    instance: table_instance.clone(),
                                    function: setter_name.0.clone(),
                                    args: vec![value],
                                });
                                expected_result.push((
                                    ExpectedResult::null_or_void(id, position, Some(setter_name)),
                                    snoozed.clone(),
                                ));
                            }
                            let id = Id::new();
                            instructions.push(Instruction::Call {
                                id: id.clone(),
                                instance: table_instance.clone(),
                                function: "execute".into(),
                                args: Vec::new(),
                            });
                            expected_result.push((
                                ExpectedResult::null_or_void(id, row.position.clone(), None),
                                snoozed.clone(),
                            ));
                            for (getter_name, Value(value, position)) in row.getters.into_iter() {
                                let id = Id::new();
                                instructions.push(Instruction::Call {
                                    id: id.clone(),
                                    instance: table_instance.clone(),
                                    function: getter_name.0.clone(),
                                    args: vec![],
                                });
                                expected_result.push((
                                    ExpectedResult::string(id, position, getter_name, value),
                                    snoozed.clone(),
                                ));
                            }
                        }
                        DecisionTableType::SingleMethod(ref method_name) => {
                            let params =
                                row.setters.into_iter().map(|setter| setter.1 .0).collect();
                            let result = row
                                .getters
                                .into_iter()
                                .map(|getter| ExpectedResultValue::String(getter.1 .0))
                                .collect();
                            let id = Id::new();
                            instructions.push(Instruction::Call {
                                id: id.clone(),
                                instance: table_instance.clone(),
                                function: method_name.0.clone(),
                                args: params,
                            });
                            expected_result.push((
                                ExpectedResult::list(id, row.position, method_name.clone(), result),
                                snoozed.clone(),
                            ));
                        }
                    }
                }
                let id = Id::new();
                instructions.push(Instruction::Call {
                    id: id.clone(),
                    instance: table_instance.clone(),
                    function: "endTable".into(),
                    args: Vec::new(),
                });
                expected_result.push((ExpectedResult::null_or_void(id, position, None), snoozed));
            }
        }
    }
    Ok((instructions, expected_result))
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

    pub fn string(id: Id, position: Position, method_name: MethodName, value: String) -> Self {
        Self {
            id,
            position,
            method_name: Some(method_name),
            value: ExpectedResultValue::String(value),
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
    String(String),
    List(Vec<ExpectedResultValue>),
}

impl Display for ExpectedResultValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(test)]
            ExpectedResultValue::Any => write!(f, "ANY"),
            ExpectedResultValue::NullOrVoid => write!(f, "NULL or VOID"),
            ExpectedResultValue::Ok => write!(f, "OK"),
            ExpectedResultValue::String(value) => write!(f, "`{}`", value),
            ExpectedResultValue::List(value) => {
                write!(
                    f,
                    "[{}]",
                    value
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<String>>()
                        .join(",")
                )
            }
        }
    }
}

#[cfg(test)]
mod test {
    use anyhow::bail;
    use chrono::NaiveDate;

    use crate::processor::markdown_commands::{DecisionTableType, Snooze, TableRow};

    use super::*;

    #[test]
    fn import() -> Result<()> {
        let (instructions, expected_result) =
            get_instructions_from_commands(vec![MarkdownCommand::Import {
                path: "Fixtures".into(),
                position: Position::new(0, 0),
            }])?;
        assert_eq!(1, instructions.len());
        assert_eq!(instructions.len(), expected_result.len());
        assert!(
            matches!(&instructions[0], Instruction::Import { id:_, path } if path == "Fixtures"),
        );
        assert!(matches!(
            &expected_result[0].0.value,
            ExpectedResultValue::Ok
        ),);

        Ok(())
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
                snoozed: Snooze::not_snooze(),
            }])?;
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
        assert!(args.is_empty());

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
                    value: ExpectedResultValue::NullOrVoid
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
                    value: ExpectedResultValue::NullOrVoid
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
            } if *expected_instance == instance && function == "setA" && args == &["1".to_string()]
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
            } if *expected_instance == instance && function == "setB" && args == &["2".to_string()]
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
                    value: ExpectedResultValue::NullOrVoid
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
                    value: ExpectedResultValue::NullOrVoid
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
            } if *expected_instance == instance && function == "setA" && args == &["3".to_string()]
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
            } if *expected_instance == instance && function == "setB" && args == &["4".to_string()]
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
                    value: ExpectedResultValue::NullOrVoid
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
                    value: ExpectedResultValue::NullOrVoid
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
                snoozed: Snooze::snooze(NaiveDate::from_ymd_opt(2099, 12, 31).unwrap()),
            }])?;
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
        assert!(args.is_empty());
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
            (ExpectedResult { id: _, position:_, method_name: None, value: ExpectedResultValue::NullOrVoid }, snooze) if snooze.should_snooze()
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
            (ExpectedResult { id: _, position:_, method_name: None, value: ExpectedResultValue::NullOrVoid }, snooze) if snooze.should_snooze()
        ));
        assert!(matches!(
            instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setA" && args == &["1".to_string()]
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
            (ExpectedResult { id: _, position:_, method_name: None, value: ExpectedResultValue::NullOrVoid }, snooze) if snooze.should_snooze()
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
            (ExpectedResult { id: _, position:_, method_name: None, value: ExpectedResultValue::NullOrVoid }, snooze) if snooze.should_snooze()
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
                snoozed: Snooze::not_snooze(),
            }])?;
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
        assert!(args.is_empty());

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
                    value: ExpectedResultValue::NullOrVoid
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
                    value: ExpectedResultValue::NullOrVoid
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
            } if *expected_instance == instance && function == "Method" && args == &["1".to_string(), "2".to_string()]
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
                    value: ExpectedResultValue::NullOrVoid
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
            } if *expected_instance == instance && function == "Method" && args == &["3".to_string(), "4".to_string()]
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
                    value: ExpectedResultValue::NullOrVoid
                },
                _
            )
        ));
        Ok(())
    }
}
