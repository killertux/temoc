use std::fmt::Display;

use anyhow::{bail, Result};

use slim_protocol::{InstructionResult, InstructionResultValue};

use super::{
    markdown_commands::Snooze,
    slim_instructions_from_commands::{ExpectedResult, ExpectedResultValue},
};

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
        if expected_result.id != result.id {
            failures.push((
                format!(
                    "Different ID in response. Expected {} but got {}",
                    expected_result.id, result.id
                ),
                snooze.clone(),
            ));
            continue;
        }
        if expected_result.value != result.value {
            failures.push((
                format!(
                    "Expected {}, got {} {}",
                    expected_result.value,
                    result.value,
                    failure_expected_result_detail_message(&file_path, &expected_result)
                ),
                snooze.clone(),
            ));
        }
    }
    Ok(failures)
}

impl PartialEq<InstructionResultValue> for ExpectedResultValue {
    fn eq(&self, other: &InstructionResultValue) -> bool {
        match (self, other) {
            (ExpectedResultValue::Any, _) => true,
            (ExpectedResultValue::NullOrVoid, InstructionResultValue::Void) => true,
            (ExpectedResultValue::NullOrVoid, InstructionResultValue::String(value))
                if value.to_lowercase() == "null" =>
            {
                true
            }
            (ExpectedResultValue::Ok, InstructionResultValue::Ok) => true,
            (
                ExpectedResultValue::String(expected_value),
                InstructionResultValue::String(actual_value),
            ) if expected_value == actual_value => true,
            (
                ExpectedResultValue::List(expected_list),
                InstructionResultValue::List(actual_list),
            ) => {
                if expected_list.len() != actual_list.len() {
                    return false;
                }
                for (a, b) in expected_list.iter().zip(actual_list.iter()) {
                    if a != b {
                        return false;
                    };
                }
                true
            }
            _ => false,
        }
    }
}

fn failure_expected_result_detail_message(
    file_path: impl Display,
    expected_result: &ExpectedResult,
) -> String {
    let position = &expected_result.position;
    let method_name = &expected_result.method_name;
    match expected_result.value {
        ExpectedResultValue::Any => "".into(),
        ExpectedResultValue::Ok => format!("in {file_path}:{position}"),
        ExpectedResultValue::NullOrVoid
        | ExpectedResultValue::String(_)
        | ExpectedResultValue::List(_) => match method_name {
            Some(method_name) => format!(
                "in {file_path}:{position} for method call {}",
                method_name.0
            ),
            None => format!("in {file_path}:{position}"),
        },
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
            vec![InstructionResult::ok(Id::new())],
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
                    ExpectedResult::null_or_void(id.clone(), position.clone(), method_name.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::null_or_void(id.clone(), position.clone(), method_name.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::ok(id.clone(), position.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::string(
                        id.clone(),
                        position.clone(),
                        method_name.clone(),
                        "Value".into(),
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::any(id.clone(), position.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::any(id.clone(), position.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::any(id.clone(), position.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::list(
                        id.clone(),
                        position.clone(),
                        method_name.clone(),
                        vec![
                            ExpectedResultValue::String("Value1".into()),
                            ExpectedResultValue::String("Value2".into()),
                        ],
                    ),
                    Snooze::not_snooze(),
                ),
            ],
            vec![
                InstructionResult::void(id.clone()),
                InstructionResult::string(id.clone(), "NULL".into()),
                InstructionResult::ok(id.clone()),
                InstructionResult::string(id.clone(), "Value".into()),
                InstructionResult::void(id.clone()),
                InstructionResult::ok(id.clone()),
                InstructionResult::string(id.clone(), "Value".into()),
                InstructionResult::list(
                    id.clone(),
                    vec![
                        InstructionResultValue::String("Value1".into()),
                        InstructionResultValue::String("Value2".into()),
                    ],
                ),
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
                    ExpectedResult::null_or_void(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::null_or_void(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::null_or_void(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::ok(id_1.clone(), position.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::ok(id_1.clone(), position.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::ok(id_1.clone(), position.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::string(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        "Value".into(),
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::string(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        "Value".into(),
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::string(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        "Value".into(),
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::string(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        "Value".into(),
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::any(id_1.clone(), position.clone()),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::list(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        vec![],
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::list(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        vec![],
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::list(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        vec![],
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::list(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        vec![],
                    ),
                    Snooze::not_snooze(),
                ),
                (
                    ExpectedResult::list(
                        id_1.clone(),
                        position.clone(),
                        method_name.clone(),
                        vec![
                            ExpectedResultValue::String("Value1".into()),
                            ExpectedResultValue::String("Value2".into()),
                        ],
                    ),
                    Snooze::not_snooze(),
                ),
            ],
            vec![
                InstructionResult::void(id_2.clone()),
                InstructionResult::ok(id_1.clone()),
                InstructionResult::string(id_1.clone(), "Value".into()),
                InstructionResult::void(id_1.clone()),
                InstructionResult::ok(id_2.clone()),
                InstructionResult::string(id_1.clone(), "Value".into()),
                InstructionResult::void(id_1.clone()),
                InstructionResult::ok(id_1.clone()),
                InstructionResult::string(id_2.clone(), "Value".into()),
                InstructionResult::string(id_1.clone(), "WrongValue".into()),
                InstructionResult::void(id_2.clone()),
                InstructionResult::list(id_2.clone(), vec![]),
                InstructionResult::void(id_1.clone()),
                InstructionResult::ok(id_1.clone()),
                InstructionResult::string(id_1.clone(), "Value".into()),
                InstructionResult::list(
                    id_1.clone(),
                    vec![
                        InstructionResultValue::String("Value2".into()),
                        InstructionResultValue::String("Value1".into()),
                    ],
                ),
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
                (format!("Different ID in response. Expected {id_1} but got {id_2}",), Snooze::not_snooze()),
                (format!(
                    "Expected [], got VOID in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected [], got OK in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected [], got `Value` in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
                (format!(
                    "Expected [`Value1`,`Value2`], got [`Value2`,`Value1`] in test_file.md:{position} for method call TestMethod"
                ), Snooze::not_snooze()),
            ],
            result
        );
        Ok(())
    }
}
