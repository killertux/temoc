use std::fmt::Display;

use anyhow::{bail, Result};

use slim_protocol::InstructionResult;

use super::{markdown_commands::Snooze, slim_instructions_from_commands::ExpectedResult};

// TODO: Clean this method. It does have a lot of repetition
pub fn validate_result(
    file_path: impl Display,
    expected_result: Vec<(ExpectedResult, Snooze)>,
    result: Vec<InstructionResult>,
) -> Result<Vec<(String, Snooze)>> {
    let mut failures = Vec::new();
    if expected_result.len() != result.len() {
        bail!("Number of instruction results `{}` does not matched the expected number of results `{}`", result.len(), expected_result.len())
    }
    for (result, (expected_result, snooze)) in result.into_iter().zip(expected_result.into_iter()) {
        match expected_result {
            ExpectedResult::NullOrVoid {
                id: expected_id,
                method_name,
                position,
            } => match result {
                InstructionResult::Void { id } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                }
                InstructionResult::Ok { id } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    failures.push((
                        format!(
                            "Expected NULL, got OK in {file_path}:{position} for method call {}",
                            method_name.0
                        ),
                        snooze.clone(),
                    ));
                }
                InstructionResult::String { id, value } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    if value.to_lowercase() != "null" {
                        failures.push((
                            format!(
                            "Expected NULL, got `{value}` in {file_path}:{position} for method call {}",
                            method_name.0
                        ),
                            snooze.clone(),
                        ));
                    }
                    continue;
                }
                InstructionResult::Exception { id, message } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    failures.push((
                        format!("Expected OK, got Exception `{}` in {file_path}:{position} for method call {}", message.pretty_message()?, method_name.0),
                        snooze.clone(),
                    ));
                }
            },
            ExpectedResult::Ok {
                id: expected_id,
                position,
            } => match result {
                InstructionResult::Void { id } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    failures.push((
                        format!("Expected OK, got NULL in {file_path}:{position}"),
                        snooze.clone(),
                    ));
                }
                InstructionResult::Ok { id } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                }
                InstructionResult::String { id, value } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    failures.push((
                        format!("Expected OK, got `{value}` in {file_path}:{position}"),
                        snooze.clone(),
                    ));
                }
                InstructionResult::Exception { id, message } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    failures.push((
                        format!(
                            "Expected OK, got Exception `{}` in {file_path}:{position}",
                            message.pretty_message()?
                        ),
                        snooze.clone(),
                    ));
                }
            },
            ExpectedResult::String {
                id: expected_id,
                value: expected_value,
                method_name,
                position,
            } => match result {
                InstructionResult::Void { id } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    failures.push((format!(
                        "Expected `{expected_value}`, got NULL in {file_path}:{position} for method call {}",
                        method_name.0
                    ), snooze.clone()));
                }
                InstructionResult::Ok { id } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    failures.push((format!(
                        "Expected `{expected_value}`, got OK in {file_path}:{position} for method call {}",
                        method_name.0
                    ), snooze.clone()));
                }
                InstructionResult::String { id, value } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    if expected_value != value {
                        failures.push((format!(
                            "Expected `{expected_value}`, got `{value}` in {file_path}:{position} for method call {}",
                            method_name.0
                        ), snooze.clone()));
                    }
                }
                InstructionResult::Exception { id, message } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze.clone(),
                        ));
                        continue;
                    }
                    failures.push((
                        format!("Expected OK, got Exception `{}` in {file_path}:{position} for method call {}", message.pretty_message()?, method_name.0),
                        snooze.clone(),
                    ));
                }
            },
            ExpectedResult::Any { id: expected_id } => match result {
                InstructionResult::Void { id }
                | InstructionResult::Ok { id }
                | InstructionResult::Exception { id, message: _ }
                | InstructionResult::String { id, value: _ } => {
                    if id != expected_id {
                        failures.push((
                            format!(
                                "Different ID in response. Expected {expected_id} but got {id}"
                            ),
                            snooze,
                        ));
                    }
                }
            },
        }
    }
    Ok(failures)
}

#[cfg(test)]
mod test {
    use crate::processor::markdown_commands::{MethodName, Position};
    use slim_protocol::Id;

    use super::*;

    #[test]
    fn incorrect_number_of_results() -> Result<()> {
        let result = validate_result(
            "file_path.md",
            vec![],
            vec![InstructionResult::Ok { id: Id::new() }],
        );
        assert_eq!(
            "Number of instruction results `1` does not matched the expected number of results `0`",
            result.expect_err("Expect error").to_string()
        );
        Ok(())
    }

    #[test]
    fn validate_no_errors() -> Result<()> {
        let id = Id::new();
        let position = Position::new(0, 0);
        let method_name = MethodName("TestMethod".into(), position.clone());
        let result = validate_result(
            "test_path.md",
            vec![
                (
                    ExpectedResult::NullOrVoid {
                        id: id.clone(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::NullOrVoid {
                        id: id.clone(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::Ok {
                        id: id.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::String {
                        id: id.clone(),
                        value: "Value".into(),
                        method_name,
                        position,
                    },
                    Snooze::not_snooze(),
                ),
                (ExpectedResult::Any { id: id.clone() }, Snooze::not_snooze()),
                (ExpectedResult::Any { id: id.clone() }, Snooze::not_snooze()),
                (ExpectedResult::Any { id: id.clone() }, Snooze::not_snooze()),
            ],
            vec![
                InstructionResult::Void { id: id.clone() },
                InstructionResult::String {
                    id: id.clone(),
                    value: "NULL".into(),
                },
                InstructionResult::Ok { id: id.clone() },
                InstructionResult::String {
                    id: id.clone(),
                    value: "Value".into(),
                },
                InstructionResult::Void { id: id.clone() },
                InstructionResult::Ok { id: id.clone() },
                InstructionResult::String {
                    id: id.clone(),
                    value: "Value".into(),
                },
            ],
        )?;
        assert!(result.is_empty());
        Ok(())
    }

    #[test]
    fn validate_erros() -> Result<()> {
        let id_1 = Id::new();
        let id_2 = Id::new();
        let position = Position::new(0, 0);
        let method_name = MethodName("TestMethod".into(), position.clone());
        let result = validate_result(
            "test_file.md",
            vec![
                (
                    ExpectedResult::NullOrVoid {
                        id: id_1.clone(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::NullOrVoid {
                        id: id_1.clone(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::NullOrVoid {
                        id: id_1.clone(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::Ok {
                        id: id_1.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::Ok {
                        id: id_1.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::Ok {
                        id: id_1.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::String {
                        id: id_1.clone(),
                        value: "Value".into(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::String {
                        id: id_1.clone(),
                        value: "Value".into(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::String {
                        id: id_1.clone(),
                        value: "Value".into(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::String {
                        id: id_1.clone(),
                        value: "Value".into(),
                        method_name: method_name.clone(),
                        position: position.clone(),
                    },
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::Any { id: id_1.clone() },
                    Snooze::not_snooze(),
                ),
            ],
            vec![
                InstructionResult::Void { id: id_2.clone() },
                InstructionResult::Ok { id: id_1.clone() },
                InstructionResult::String {
                    id: id_1.clone(),
                    value: "Value".into(),
                },
                InstructionResult::Void { id: id_1.clone() },
                InstructionResult::Ok { id: id_2.clone() },
                InstructionResult::String {
                    id: id_1.clone(),
                    value: "Value".into(),
                },
                InstructionResult::Void { id: id_1.clone() },
                InstructionResult::Ok { id: id_1.clone() },
                InstructionResult::String {
                    id: id_2.clone(),
                    value: "Value".into(),
                },
                InstructionResult::String {
                    id: id_1.clone(),
                    value: "WrongValue".into(),
                },
                InstructionResult::Void { id: id_2.clone() },
            ],
        )?;
        assert_eq!(
            vec![
                (format!("Different ID in response. Expected {id_1} but got {id_2}",), Snooze::not_snooze()),
                (format!(
                    "Expected NULL, got OK in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected NULL, got `Value` in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected OK, got NULL in test_file.md:{position}"
                ), Snooze::not_snooze()),
                (format!("Different ID in response. Expected {id_1} but got {id_2}",),Snooze::not_snooze()),
                (format!(
                    "Expected OK, got `Value` in test_file.md:{position}"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected `Value`, got NULL in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected `Value`, got OK in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!("Different ID in response. Expected {id_1} but got {id_2}",), Snooze::not_snooze()),
                (format!(
                    "Expected `Value`, got `WrongValue` in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!("Different ID in response. Expected {id_1} but got {id_2}",), Snooze::not_snooze()),
            ],
            result
        );
        Ok(())
    }
}
