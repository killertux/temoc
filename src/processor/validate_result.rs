use std::fmt::Display;

use anyhow::{bail, Result};

use crate::slim::InstructionResult;

use super::slim_instructions_from_commands::ExpectedResult;

// TODO: Clean this method. It does have a lot of repetition
pub fn validate_result(
    file_path: impl Display,
    expected_result: Vec<ExpectedResult>,
    result: Vec<InstructionResult>,
) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    if expected_result.len() != result.len() {
        bail!("Number of instruction results `{}` does not matched the expected number of results `{}`", result.len(), expected_result.len())
    }
    for (result, expected_result) in result.into_iter().zip(expected_result.into_iter()) {
        match expected_result {
            ExpectedResult::Null {
                id: expected_id,
                method_name,
                position,
            } => match result {
                InstructionResult::Null { id } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                }
                InstructionResult::Ok { id } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                    failures.push(format!(
                        "Expected NULL, got OK in {file_path}:{position} for method call {}",
                        method_name.0
                    ));
                }
                InstructionResult::String { id, value } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                    failures.push(format!(
                        "Expected NULL, got `{value}` in {file_path}:{position} for method call {}",
                        method_name.0
                    ));
                }
            },
            ExpectedResult::Ok {
                id: expected_id,
                position,
            } => match result {
                InstructionResult::Null { id } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                    failures.push(format!("Expected OK, got NULL in {file_path}:{position}"));
                }
                InstructionResult::Ok { id } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                }
                InstructionResult::String { id, value } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                    failures.push(format!(
                        "Expected OK, got `{value}` in {file_path}:{position}"
                    ));
                }
            },
            ExpectedResult::String {
                id: expected_id,
                value: expected_value,
                method_name,
                position,
            } => match result {
                InstructionResult::Null { id } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                    failures.push(format!(
                        "Expected `{expected_value}`, got NULL in {file_path}:{position} for method call {}",
                        method_name.0
                    ));
                }
                InstructionResult::Ok { id } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                    failures.push(format!(
                        "Expected `{expected_value}`, got OK in {file_path}:{position} for method call {}",
                        method_name.0
                    ));
                }
                InstructionResult::String { id, value } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
                        ));
                        continue;
                    }
                    if expected_value != value {
                        failures.push(format!(
                            "Expected `{expected_value}`, got `{value}` in {file_path}:{position} for method call {}",
                            method_name.0
                        ));
                    }
                }
            },
            ExpectedResult::Any { id: expected_id } => match result {
                InstructionResult::Null { id }
                | InstructionResult::Ok { id }
                | InstructionResult::String { id, value: _ } => {
                    if id != expected_id {
                        failures.push(format!(
                            "Different ID in response. Expected {expected_id} but got {id}"
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
    use crate::{
        processor::markdown_commands::{MethodName, Position},
        slim::Id,
    };

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
                ExpectedResult::Null {
                    id: id.clone(),
                    method_name: method_name.clone(),
                    position: position.clone(),
                },
                ExpectedResult::Ok {
                    id: id.clone(),
                    position: position.clone(),
                },
                ExpectedResult::String {
                    id: id.clone(),
                    value: "Value".into(),
                    method_name: method_name,
                    position: position,
                },
                ExpectedResult::Any { id: id.clone() },
                ExpectedResult::Any { id: id.clone() },
                ExpectedResult::Any { id: id.clone() },
            ],
            vec![
                InstructionResult::Null { id: id.clone() },
                InstructionResult::Ok { id: id.clone() },
                InstructionResult::String {
                    id: id.clone(),
                    value: "Value".into(),
                },
                InstructionResult::Null { id: id.clone() },
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
                ExpectedResult::Null {
                    id: id_1.clone(),
                    method_name: method_name.clone(),
                    position: position.clone(),
                },
                ExpectedResult::Null {
                    id: id_1.clone(),
                    method_name: method_name.clone(),
                    position: position.clone(),
                },
                ExpectedResult::Null {
                    id: id_1.clone(),
                    method_name: method_name.clone(),
                    position: position.clone(),
                },
                ExpectedResult::Ok {
                    id: id_1.clone(),
                    position: position.clone(),
                },
                ExpectedResult::Ok {
                    id: id_1.clone(),
                    position: position.clone(),
                },
                ExpectedResult::Ok {
                    id: id_1.clone(),
                    position: position.clone(),
                },
                ExpectedResult::String {
                    id: id_1.clone(),
                    value: "Value".into(),
                    method_name: method_name.clone(),
                    position: position.clone(),
                },
                ExpectedResult::String {
                    id: id_1.clone(),
                    value: "Value".into(),
                    method_name: method_name.clone(),
                    position: position.clone(),
                },
                ExpectedResult::String {
                    id: id_1.clone(),
                    value: "Value".into(),
                    method_name: method_name.clone(),
                    position: position.clone(),
                },
                ExpectedResult::String {
                    id: id_1.clone(),
                    value: "Value".into(),
                    method_name: method_name.clone(),
                    position: position.clone(),
                },
                ExpectedResult::Any { id: id_1.clone() },
            ],
            vec![
                InstructionResult::Null { id: id_2.clone() },
                InstructionResult::Ok { id: id_1.clone() },
                InstructionResult::String {
                    id: id_1.clone(),
                    value: "Value".into(),
                },
                InstructionResult::Null { id: id_1.clone() },
                InstructionResult::Ok { id: id_2.clone() },
                InstructionResult::String {
                    id: id_1.clone(),
                    value: "Value".into(),
                },
                InstructionResult::Null { id: id_1.clone() },
                InstructionResult::Ok { id: id_1.clone() },
                InstructionResult::String {
                    id: id_2.clone(),
                    value: "Value".into(),
                },
                InstructionResult::String {
                    id: id_1.clone(),
                    value: "WrongValue".into(),
                },
                InstructionResult::Null { id: id_2.clone() },
            ],
        )?;
        assert_eq!(
            vec![
                format!("Different ID in response. Expected {id_1} but got {id_2}",),
                format!(
                    "Expected NULL, got OK in test_file.md:{position} for method call TestMethod"
                ),
                format!(
                    "Expected NULL, got `Value` in test_file.md:{position} for method call TestMethod"
                ),
                format!(
                    "Expected OK, got NULL in test_file.md:{position}"
                ),
                format!("Different ID in response. Expected {id_1} but got {id_2}",),
                format!(
                    "Expected OK, got `Value` in test_file.md:{position}"
                ),
                format!(
                    "Expected `Value`, got NULL in test_file.md:{position} for method call TestMethod"
                ),
                format!(
                    "Expected `Value`, got OK in test_file.md:{position} for method call TestMethod"
                ),
                format!("Different ID in response. Expected {id_1} but got {id_2}",),
                format!(
                    "Expected `Value`, got `WrongValue` in test_file.md:{position} for method call TestMethod"
                ),
                format!("Different ID in response. Expected {id_1} but got {id_2}",),
            ],
            result
        );
        Ok(())
    }
}
