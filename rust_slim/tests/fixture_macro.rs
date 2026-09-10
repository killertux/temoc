#![cfg(feature = "macros")]

use rust_slim::{
    fixture, ClassPath, ExecuteMethodError, FromSlimValue, IntoSlimValue, SlimFixture, SlimValue,
};
use std::marker::PhantomData;

#[derive(Default)]
struct MacroFixture;

struct Word(String);

struct GenericFixture<T>(PhantomData<T>);

#[fixture]
impl<T> GenericFixture<T> {
    pub fn value(&self) -> &'static str {
        "generic"
    }
}

impl FromSlimValue for Word {
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
        match value {
            SlimValue::String(value) => Ok(Self(value.to_uppercase())),
            _ => Err(ExecuteMethodError::ArgumentParsingError(
                "expected a word".into(),
            )),
        }
    }
}

impl IntoSlimValue for Word {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        Ok(SlimValue::String(self.0))
    }
}

#[fixture("Test.MacroFixture")]
impl MacroFixture {
    pub fn helper() -> &'static str {
        "associated functions are not fixture methods"
    }

    pub fn nested(&self, values: Vec<Vec<i64>>) -> Vec<Vec<i64>> {
        values
    }

    pub fn custom(&self, value: Word) -> Word {
        value
    }

    pub fn fails(&self) -> Result<(), &'static str> {
        Err("fixture failed")
    }
}

#[test]
fn macro_converts_nested_values_and_custom_types() {
    let mut fixture = MacroFixture;
    assert_eq!(
        Ok(SlimValue::List(vec![
            SlimValue::List(vec![SlimValue::String("1".into())]),
            SlimValue::List(vec![
                SlimValue::String("2".into()),
                SlimValue::String("3".into())
            ]),
        ])),
        fixture.execute_method(
            "nested",
            vec![SlimValue::List(vec![
                SlimValue::List(vec![SlimValue::String("1".into())]),
                SlimValue::List(vec![
                    SlimValue::String("2".into()),
                    SlimValue::String("3".into())
                ]),
            ])],
        )
    );
    assert_eq!(
        Ok(SlimValue::String("HELLO".into())),
        fixture.execute_method("custom", vec![SlimValue::String("hello".into())])
    );
}

#[test]
fn macro_checks_arity_before_accessing_arguments() {
    let mut fixture = MacroFixture;
    assert_eq!(
        Err(ExecuteMethodError::MethodNotFound {
            method: "custom".into(),
            class: "Test.MacroFixture".into(),
        }),
        fixture.execute_method("custom", Vec::new())
    );
    assert_eq!(
        "associated functions are not fixture methods",
        MacroFixture::helper()
    );
}

#[test]
fn macro_turns_result_errors_into_execution_errors() {
    let mut fixture = MacroFixture;
    assert_eq!(
        Err(ExecuteMethodError::ExecutionError("fixture failed".into())),
        fixture.execute_method("fails", Vec::new())
    );
}

#[test]
fn macro_supports_generic_fixture_types() {
    let mut fixture = GenericFixture::<i64>(PhantomData);
    assert_eq!(
        Ok(SlimValue::String("generic".into())),
        fixture.execute_method("value", Vec::new())
    );
    assert!(GenericFixture::<i64>::class_path().ends_with("GenericFixture"));
}
