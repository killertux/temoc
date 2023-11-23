use anyhow::Result;
use ulid::Ulid;

use slim_protocol::{Id, Instruction};

use super::markdown_commands::{Class, MarkdownCommand, MethodName, Position, Snooze, Value};

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
                expected_result.push((ExpectedResult::Ok { id, position }, Snooze::not_snooze()))
            }
            MarkdownCommand::DecisionTable {
                class: Class(test_class, position),
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
                expected_result.push((ExpectedResult::Ok { id, position }, snoozed.clone()));
                let id = Id::new();
                instructions.push(Instruction::Call {
                    id: id.clone(),
                    instance: table_instance.clone(),
                    function: "beginTable".into(),
                    args: Vec::new(),
                });
                expected_result.push((ExpectedResult::Any { id }, snoozed.clone()));
                for row in table.into_iter() {
                    let id = Id::new();
                    instructions.push(Instruction::Call {
                        id: id.clone(),
                        instance: table_instance.clone(),
                        function: "reset".into(),
                        args: Vec::new(),
                    });
                    expected_result.push((ExpectedResult::Any { id }, snoozed.clone()));
                    for (setter_name, Value(value, position)) in row.setters.into_iter() {
                        let id = Id::new();
                        instructions.push(Instruction::Call {
                            id: id.clone(),
                            instance: table_instance.clone(),
                            function: setter_name.0.clone(),
                            args: vec![value],
                        });
                        expected_result.push((
                            ExpectedResult::Null {
                                id,
                                method_name: setter_name,
                                position,
                            },
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
                    expected_result.push((ExpectedResult::Any { id }, snoozed.clone()));
                    for (getter_name, Value(value, position)) in row.getters.into_iter() {
                        let id = Id::new();
                        instructions.push(Instruction::Call {
                            id: id.clone(),
                            instance: table_instance.clone(),
                            function: getter_name.0.clone(),
                            args: vec![],
                        });
                        expected_result.push((
                            ExpectedResult::String {
                                id,
                                value,
                                method_name: getter_name,
                                position,
                            },
                            snoozed.clone(),
                        ));
                    }
                }
                let id = Id::new();
                instructions.push(Instruction::Call {
                    id: id.clone(),
                    instance: table_instance.clone(),
                    function: "endTable".into(),
                    args: Vec::new(),
                });
                expected_result.push((ExpectedResult::Any { id }, snoozed));
            }
        }
    }
    Ok((instructions, expected_result))
}

#[derive(PartialEq, Eq, Debug)]
pub enum ExpectedResult {
    Ok {
        id: Id,
        position: Position,
    },
    Null {
        id: Id,
        method_name: MethodName,
        position: Position,
    },
    String {
        id: Id,
        value: String,
        method_name: MethodName,
        position: Position,
    },
    Any {
        id: Id,
    },
}

#[cfg(test)]
mod test {
    use anyhow::bail;
    use chrono::NaiveDate;

    use crate::processor::markdown_commands::{Snooze, TableRow};

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
            &expected_result[0].0,
            ExpectedResult::Ok { id: _, position: _ }
        ),);

        Ok(())
    }

    #[test]
    fn decision_table() -> Result<()> {
        let position = Position::new(0, 0);
        let (mut instructions, mut expected_result) =
            get_instructions_from_commands(vec![MarkdownCommand::DecisionTable {
                class: Class("Class".into(), position.clone()),
                table: vec![
                    TableRow {
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
            matches!(expected_result.remove(0), (ExpectedResult::Ok { id: expected_id, position:_ }, snooze) if expected_id == id && !snooze.should_snooze())
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
            (ExpectedResult::Any { id: _ }, _)
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
            (ExpectedResult::Any { id: _ }, _)
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
            (ExpectedResult::Null { id:_, method_name, position:_ }, _) if method_name.0 == "setA"
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
            (ExpectedResult::Null { id:_, method_name, position:_ }, _) if method_name.0 == "setB"
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
            (ExpectedResult::Any { id: _ }, _)
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
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
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
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
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
            (ExpectedResult::Any { id: _ }, _)
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
            (ExpectedResult::Null { id:_, method_name, position:_ }, _) if method_name.0 == "setA"
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
            (ExpectedResult::Null { id:_, method_name, position:_ }, _) if method_name.0 == "setB"
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
            (ExpectedResult::Any { id: _ }, _)
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
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
                position: _
            },_) if method_name.0 == "getA" && value == "3"
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
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
                position: _
            },_) if method_name.0 == "getB" && value == "4"
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
            (ExpectedResult::Any { id: _ }, _)
        ));
        Ok(())
    }

    #[test]
    fn snooze() -> Result<()> {
        let position = Position::new(0, 0);
        let (mut instructions, mut expected_result) =
            get_instructions_from_commands(vec![MarkdownCommand::DecisionTable {
                class: Class("Class".into(), position.clone()),
                table: vec![TableRow {
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
            matches!(expected_result.remove(0), (ExpectedResult::Ok { id: expected_id, position:_ }, snooze) if expected_id == id && snooze.should_snooze())
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
            (ExpectedResult::Any { id: _ }, snooze) if snooze.should_snooze()
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
            (ExpectedResult::Any { id: _ }, snooze) if snooze.should_snooze()
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
            (ExpectedResult::Null { id:_, method_name, position:_ }, snooze) if method_name.0 == "setA" && snooze.should_snooze()
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
            (ExpectedResult::Any { id: _ }, snooze) if snooze.should_snooze()
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
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
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
            (ExpectedResult::Any { id: _ }, snooze) if snooze.should_snooze()
        ));
        Ok(())
    }
}
