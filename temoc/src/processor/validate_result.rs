use std::fmt::Display;

use anyhow::{bail, Result};

use slim_protocol::{Id, InstructionResult};

use super::{markdown_commands::Snooze, slim_instructions_from_commands::ExpectedResult};

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
        if expected_result.get_id() != result.get_id() {
            failures.push((
                format!(
                    "Different ID in response. Expected {} but got {}",
                    expected_result.get_id(),
                    result.get_id()
                ),
                snooze.clone(),
            ));
            continue;
        }
        if expected_result != result {
            failures.push((
                format!(
                    "Expected {}, got {} {}",
                    expected_result,
                    result,
                    failure_expected_result_detail_message(&file_path, &expected_result)
                ),
                snooze.clone(),
            ));
        }
    }
    Ok(failures)
}

trait GetId {
    fn get_id(&self) -> &Id;
}

impl GetId for ExpectedResult {
    fn get_id(&self) -> &Id {
        match self {
            ExpectedResult::Ok { id, position: _ } => id,
            ExpectedResult::Any { id } => id,
            ExpectedResult::String {
                id,
                value: _,
                method_name: _,
                position: _,
            } => id,
            ExpectedResult::NullOrVoid {
                id,
                method_name: _,
                position: _,
            } => id,
        }
    }
}

impl GetId for InstructionResult {
    fn get_id(&self) -> &Id {
        match self {
            InstructionResult::Ok { id } => id,
            InstructionResult::String { id, value: _ } => id,
            InstructionResult::Void { id } => id,
            InstructionResult::Exception { id, message: _ } => id,
        }
    }
}

impl PartialEq<InstructionResult> for ExpectedResult {
    fn eq(&self, other: &InstructionResult) -> bool {
        match (self, other) {
            (ExpectedResult::Any { id: _ }, _) => true,
            (
                ExpectedResult::NullOrVoid {
                    id: _,
                    method_name: _,
                    position: _,
                },
                InstructionResult::Void { id: _ },
            ) => true,
            (
                ExpectedResult::NullOrVoid {
                    id: _,
                    method_name: _,
                    position: _,
                },
                InstructionResult::String { id: _, value },
            ) if value.to_lowercase() == "null" => true,
            (
                ExpectedResult::NullOrVoid {
                    id: _,
                    method_name: _,
                    position: _,
                },
                _,
            ) => false,
            (ExpectedResult::Ok { id: _, position: _ }, InstructionResult::Ok { id: _ }) => true,
            (ExpectedResult::Ok { id: _, position: _ }, _) => false,
            (
                ExpectedResult::String {
                    id: _,
                    value: expected_value,
                    method_name: _,
                    position: _,
                },
                InstructionResult::String {
                    id: _,
                    value: actual_value,
                },
            ) if expected_value == actual_value => true,
            _ => false,
        }
    }
}

fn failure_expected_result_detail_message(
    file_path: impl Display,
    expected_result: &ExpectedResult,
) -> String {
    match expected_result {
        ExpectedResult::Any { id: _ } => "".into(),
        ExpectedResult::Ok { id: _, position } => format!("in {file_path}:{position}"),
        ExpectedResult::NullOrVoid {
            id: _,
            method_name,
            position,
        }
        | ExpectedResult::String {
            id: _,
            value: _,
            method_name,
            position,
        } => format!(
            "in {file_path}:{position} for method call {}",
            method_name.0
        ),
    }
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
                    "Expected NULL or VOID, got OK in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected NULL or VOID, got `Value` in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected OK, got VOID in test_file.md:{position}"
                ), Snooze::not_snooze()),
                (format!("Different ID in response. Expected {id_1} but got {id_2}",),Snooze::not_snooze()),
                (format!(
                    "Expected OK, got `Value` in test_file.md:{position}"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected `Value`, got VOID in test_file.md:{position} for method call TestMethod"
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
