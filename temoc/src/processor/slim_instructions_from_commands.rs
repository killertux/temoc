use super::markdown_commands::{Class, MarkdownCommand, MethodName, Position, Snooze, Value};
use anyhow::Result;
use slim_protocol::{Id, Instruction};
use ulid::Ulid;

pub type ExpectedResulWithSnooze = (ExpectedResult, Snooze);

pub fn get_instructions_from_commands(
    commands: Vec<MarkdownCommand>,
) -> Result<(
    Vec<(LoggerEvent, Vec<Instruction>)>,
    Vec<Vec<ExpectedResulWithSnooze>>,
)> {
    let mut instructions = Vec::new();
    let mut expected_result = Vec::new();
    let mut n_decision_table = 0;
    for command in commands {
        match command {
            MarkdownCommand::Import { path, position } => {
                let id = Id::new();
                instructions.push((
                    LoggerEvent::Nop,
                    Vec::from([Instruction::Import {
                        id: id.clone(),
                        path,
                    }]),
                ));
                expected_result.push(Vec::from([(
                    ExpectedResult::Ok { id, position },
                    Snooze::not_snooze(),
                )]))
            }
            MarkdownCommand::DecisionTable {
                class: Class(test_class, position),
                table,
                snoozed,
            } => {
                n_decision_table += 1;
                let table_instance = Ulid::new().to_string();
                let id = Id::new();
                let mut begin_table_instructions = Vec::new();
                let mut begin_table_results = Vec::new();
                begin_table_instructions.push(Instruction::Make {
                    id: id.clone(),
                    instance: table_instance.clone(),
                    class: test_class,
                    args: Vec::new(),
                });
                begin_table_results.push((ExpectedResult::Ok { id, position }, snoozed.clone()));
                let id = Id::new();
                begin_table_instructions.push(Instruction::Call {
                    id: id.clone(),
                    instance: table_instance.clone(),
                    function: "beginTable".into(),
                    args: Vec::new(),
                });
                begin_table_results.push((ExpectedResult::Any { id }, snoozed.clone()));
                instructions.push((LoggerEvent::Nop, begin_table_instructions));
                expected_result.push(begin_table_results);
                let decision_table_name = format!("Decision Table {n_decision_table}");
                for (row_number, row) in table.into_iter().enumerate() {
                    let mut row_instructions = Vec::new();
                    let mut row_results = Vec::new();
                    let id = Id::new();
                    row_instructions.push(Instruction::Call {
                        id: id.clone(),
                        instance: table_instance.clone(),
                        function: "reset".into(),
                        args: Vec::new(),
                    });
                    row_results.push((ExpectedResult::Any { id }, snoozed.clone()));
                    for (setter_name, Value(value, position)) in row.setters.into_iter() {
                        let id = Id::new();
                        row_instructions.push(Instruction::Call {
                            id: id.clone(),
                            instance: table_instance.clone(),
                            function: setter_name.0.clone(),
                            args: vec![value],
                        });
                        row_results.push((
                            ExpectedResult::NullOrVoid {
                                id,
                                method_name: setter_name,
                                position,
                            },
                            snoozed.clone(),
                        ));
                    }
                    let id = Id::new();
                    row_instructions.push(Instruction::Call {
                        id: id.clone(),
                        instance: table_instance.clone(),
                        function: "execute".into(),
                        args: Vec::new(),
                    });
                    row_results.push((ExpectedResult::Any { id }, snoozed.clone()));
                    for (getter_name, Value(value, position)) in row.getters.into_iter() {
                        let id = Id::new();
                        row_instructions.push(Instruction::Call {
                            id: id.clone(),
                            instance: table_instance.clone(),
                            function: getter_name.0.clone(),
                            args: vec![],
                        });
                        row_results.push((
                            ExpectedResult::String {
                                id,
                                value,
                                method_name: getter_name,
                                position,
                            },
                            snoozed.clone(),
                        ));
                    }
                    instructions.push((
                        LoggerEvent::RowExecution {
                            decision_table_name: decision_table_name.clone(),
                            row_number: row_number + 1,
                            position: row.position,
                        },
                        row_instructions,
                    ));
                    expected_result.push(row_results);
                }
                let id = Id::new();
                instructions.push((
                    LoggerEvent::Nop,
                    Vec::from([Instruction::Call {
                        id: id.clone(),
                        instance: table_instance.clone(),
                        function: "endTable".into(),
                        args: Vec::new(),
                    }]),
                ));
                expected_result.push(Vec::from([(ExpectedResult::Any { id }, snoozed)]));
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
    NullOrVoid {
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

pub enum LoggerEvent {
    Nop,
    RowExecution {
        decision_table_name: String,
        row_number: usize,
        position: Position,
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
            matches!(&instructions[0].1[0], Instruction::Import { id:_, path } if path == "Fixtures"),
        );
        assert!(matches!(
            &expected_result[0][0].0,
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
                        position: Position::new(1, 1),
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
                        position: Position::new(2, 1),
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
        assert_eq!(4, instructions.len());
        assert_eq!(instructions.len(), expected_result.len());
        let (logger_event, mut begin_table_instructions) = instructions.remove(0);
        let mut begin_table_results = expected_result.remove(0);
        assert_eq!(begin_table_instructions.len(), begin_table_results.len());
        assert_eq!(2, begin_table_instructions.len());
        assert!(matches!(logger_event, LoggerEvent::Nop));
        let Instruction::Make {
            id,
            instance,
            class,
            args,
        } = begin_table_instructions.remove(0)
        else {
            bail!("Expected make");
        };
        assert_eq!("Class", class);
        assert!(args.is_empty());
        assert!(
            matches!(begin_table_results.remove(0), (ExpectedResult::Ok { id: expected_id, position:_ }, snooze) if expected_id == id && !snooze.should_snooze())
        );
        assert!(matches!(
            begin_table_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "beginTable" && args.is_empty()
        ));
        assert!(matches!(
            begin_table_results.remove(0),
            (ExpectedResult::Any { id: _ }, _)
        ));
        let (logger_event, mut row_instructions) = instructions.remove(0);
        let mut row_results = expected_result.remove(0);
        assert_eq!(row_instructions.len(), row_results.len());
        assert_eq!(6, row_instructions.len());
        let LoggerEvent::RowExecution {
            decision_table_name,
            row_number,
            position,
        } = logger_event
        else {
            bail!("Incorrect logger event")
        };
        assert_eq!("Decision Table 1", decision_table_name);
        assert_eq!(1, row_number);
        assert_eq!(Position::new(1, 1), position);
        assert!(matches!(
            row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            row_results.remove(0),
            (ExpectedResult::Any { id: _ }, _)
        ));
        assert!(matches!(
            row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setA" && args == &["1".to_string()]
        ));
        assert!(matches!(
            row_results.remove(0),
            (ExpectedResult::NullOrVoid { id:_, method_name, position:_ }, _) if method_name.0 == "setA"
        ));
        assert!(matches!(
            &row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setB" && args == &["2".to_string()]
        ));
        assert!(matches!(
            &row_results.remove(0),
            (ExpectedResult::NullOrVoid { id:_, method_name, position:_ }, _) if method_name.0 == "setB"
        ));
        assert!(matches!(
            row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "execute" && args.is_empty()
        ));
        assert!(matches!(
            row_results.remove(0),
            (ExpectedResult::Any { id: _ }, _)
        ));
        assert!(matches!(
            &row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getA" && args.is_empty()
        ));
        assert!(matches!(
            &row_results.remove(0),
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
                position: _
            }, _) if method_name.0 == "getA" && value == "1"
        ));
        assert!(matches!(
            &row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getB" && args.is_empty()
        ));
        assert!(matches!(
            &row_results.remove(0),
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
                position: _
            }, _) if method_name.0 == "getB" && value == "2"
        ));
        let (logger_event, mut row_instructions) = instructions.remove(0);
        let mut row_results = expected_result.remove(0);
        assert_eq!(row_instructions.len(), row_results.len());
        assert_eq!(6, row_instructions.len());
        let LoggerEvent::RowExecution {
            decision_table_name,
            row_number,
            position,
        } = logger_event
        else {
            bail!("Incorrect logger event")
        };
        assert_eq!("Decision Table 1", decision_table_name);
        assert_eq!(2, row_number);
        assert_eq!(Position::new(2, 1), position);
        assert!(matches!(
            row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            row_results.remove(0),
            (ExpectedResult::Any { id: _ }, _)
        ));
        assert!(matches!(
            &row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setA" && args == &["3".to_string()]
        ));
        assert!(matches!(
            &row_results.remove(0),
            (ExpectedResult::NullOrVoid { id:_, method_name, position:_ }, _) if method_name.0 == "setA"
        ));
        assert!(matches!(
            &row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setB" && args == &["4".to_string()]
        ));
        assert!(matches!(
            &row_results.remove(0),
            (ExpectedResult::NullOrVoid { id:_, method_name, position:_ }, _) if method_name.0 == "setB"
        ));
        assert!(matches!(
            row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "execute" && args.is_empty()
        ));
        assert!(matches!(
            row_results.remove(0),
            (ExpectedResult::Any { id: _ }, _)
        ));
        assert!(matches!(
            &row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getA" && args.is_empty()
        ));
        assert!(matches!(
            &row_results.remove(0),
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
                position: _
            },_) if method_name.0 == "getA" && value == "3"
        ));
        assert!(matches!(
            &row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getB" && args.is_empty()
        ));
        assert!(matches!(
            &row_results.remove(0),
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
                position: _
            },_) if method_name.0 == "getB" && value == "4"
        ));
        let (logger_event, mut end_table_instructions) = instructions.remove(0);
        let mut end_table_results = expected_result.remove(0);
        assert_eq!(end_table_instructions.len(), end_table_results.len());
        assert_eq!(1, end_table_instructions.len());
        assert!(matches!(logger_event, LoggerEvent::Nop));
        assert!(matches!(
            end_table_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "endTable" && args.is_empty()
        ));
        assert!(matches!(
            end_table_results.remove(0),
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
                    position: Position::new(1, 1),
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
        assert_eq!(3, instructions.len());
        assert_eq!(instructions.len(), expected_result.len());
        let (logger_event, mut begin_table_instructions) = instructions.remove(0);
        let mut begin_table_results = expected_result.remove(0);
        assert_eq!(begin_table_instructions.len(), begin_table_results.len());
        assert_eq!(2, begin_table_instructions.len());
        assert!(matches!(logger_event, LoggerEvent::Nop));
        let Instruction::Make {
            id,
            instance,
            class,
            args,
        } = begin_table_instructions.remove(0)
        else {
            bail!("Expected make");
        };
        assert_eq!("Class", class);
        assert!(args.is_empty());
        assert!(
            matches!(begin_table_results.remove(0), (ExpectedResult::Ok { id: expected_id, position:_ }, snooze) if expected_id == id && snooze.should_snooze())
        );
        assert!(matches!(
            begin_table_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "beginTable" && args.is_empty()
        ));
        assert!(matches!(
            begin_table_results.remove(0),
            (ExpectedResult::Any { id: _ }, snooze) if snooze.should_snooze()
        ));
        let (logger_event, mut row_instructions) = instructions.remove(0);
        let mut row_results = expected_result.remove(0);
        assert_eq!(4, row_instructions.len());
        assert_eq!(row_instructions.len(), row_results.len());
        let LoggerEvent::RowExecution {
            decision_table_name,
            row_number,
            position,
        } = logger_event
        else {
            bail!("Incorrect logger event")
        };
        assert_eq!("Decision Table 1", decision_table_name);
        assert_eq!(1, row_number);
        assert_eq!(Position::new(1, 1), position);
        assert!(matches!(
            row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "reset" && args.is_empty()
        ));
        assert!(matches!(
            row_results.remove(0),
            (ExpectedResult::Any { id: _ }, snooze) if snooze.should_snooze()
        ));
        assert!(matches!(
            row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "setA" && args == &["1".to_string()]
        ));
        assert!(matches!(
            row_results.remove(0),
            (ExpectedResult::NullOrVoid { id:_, method_name, position:_ }, snooze) if method_name.0 == "setA" && snooze.should_snooze()
        ));
        assert!(matches!(
            row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "execute" && args.is_empty()
        ));
        assert!(matches!(
            row_results.remove(0),
            (ExpectedResult::Any { id: _ }, snooze) if snooze.should_snooze()
        ));
        assert!(matches!(
            &row_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "getA" && args.is_empty()
        ));
        assert!(matches!(
            &row_results.remove(0),
            (ExpectedResult::String {
                id: _,
                value,
                method_name,
                position: _
            }, snooze) if method_name.0 == "getA" && value == "1" && snooze.should_snooze()
        ));
        let (logger_event, mut end_table_instructions) = instructions.remove(0);
        let mut end_table_results = expected_result.remove(0);
        assert_eq!(end_table_instructions.len(), end_table_results.len());
        assert_eq!(1, end_table_instructions.len());
        assert!(matches!(logger_event, LoggerEvent::Nop));
        assert!(matches!(
            end_table_instructions.remove(0),
            Instruction::Call {
                id: _,
                instance: expected_instance,
                function,
                args
            } if *expected_instance == instance && function == "endTable" && args.is_empty()
        ));
        assert!(matches!(
            end_table_results.remove(0),
            (ExpectedResult::Any { id: _ }, snooze) if snooze.should_snooze()
        ));
        Ok(())
    }
}
