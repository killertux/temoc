#![cfg(feature = "macros")]

use rust_slim::{
    fixture, ClassPath, Constructor, ConstructorError, ExecuteMethodError, FromSlimValue,
    IntoSlimValue, SlimControl, SlimControlException, SlimFixture, SlimValue,
};
use std::marker::PhantomData;

#[derive(Default)]
struct MacroFixture;

struct Word(String);

#[derive(Default)]
struct GenericFixture<T>(PhantomData<T>);
#[derive(Default)]
struct GenericDefaultFixture<T>(PhantomData<T>);
#[derive(Default)]
struct BoundedDefaultFixture<T>(T);

#[derive(Debug, PartialEq)]
struct Constructed(i64);
struct SutFixture {
    sut: Sut,
}
#[derive(Default)]
struct Sut;
#[derive(Debug, PartialEq)]
struct MultiConstructor(i64);
struct ControlledConstructor;

#[fixture]
impl<T> GenericFixture<T> {
    pub fn value(&self) -> &'static str {
        "generic"
    }
}

#[fixture]
impl<T> GenericDefaultFixture<T> {
    pub fn value(&self) -> &'static str {
        "default"
    }
}

#[fixture]
impl<T> BoundedDefaultFixture<T>
where
    T: Default,
{
    pub fn value(&self) -> &'static str {
        "bounded default"
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

#[fixture("Test.Constructed")]
impl Constructed {
    #[slim(constructor)]
    pub fn new(number: i64) -> Result<Self, &'static str> {
        if number >= 0 {
            Ok(Self(number))
        } else {
            Err("negative")
        }
    }

    pub fn value(&self) -> i64 {
        self.0
    }
}

#[fixture]
impl MultiConstructor {
    #[slim(constructor)]
    pub fn empty() -> Self {
        Self(0)
    }

    #[slim(constructor)]
    pub fn with_value(value: i64) -> Self {
        Self(value)
    }
}

#[fixture]
impl ControlledConstructor {
    #[slim(constructor)]
    pub fn new() -> Result<Self, SlimControlException> {
        Err(SlimControl::abort_slim_test("constructor control"))
    }
}

#[fixture]
impl Sut {
    pub fn answer(&self) -> i64 {
        42
    }
}

#[fixture]
impl SutFixture {
    #[slim(constructor)]
    pub fn new() -> Self {
        Self { sut: Sut }
    }

    #[slim(sut)]
    pub fn system_under_test(&mut self) -> &mut Sut {
        &mut self.sut
    }
}

#[fixture("Test.MacroFixture")]
impl MacroFixture {
    pub fn helper() -> &'static str {
        "associated functions are not fixture methods"
    }

    pub fn generic_helper<T>(value: T) -> T {
        value
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

    pub fn abort(&self) -> SlimControlException {
        SlimControl::abort_slim_test("macro control")
    }

    pub fn abort_result(&self) -> Result<(), SlimControlException> {
        Err(SlimControl::ignore_all_tests("macro result control"))
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
    assert_eq!(42, MacroFixture::generic_helper(42));
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
fn macro_preserves_structured_control_results() {
    let mut fixture = MacroFixture;
    assert_eq!(
        Err(ExecuteMethodError::Control(SlimControl::abort_slim_test(
            "macro control"
        ))),
        fixture.execute_method("abort", Vec::new())
    );
    assert_eq!(
        Err(ExecuteMethodError::Control(SlimControl::ignore_all_tests(
            "macro result control"
        ))),
        fixture.execute_method("abort_result", Vec::new())
    );
    assert!(matches!(
        ControlledConstructor::construct(Vec::new()),
        Err(ConstructorError::Control(control))
            if control == SlimControl::abort_slim_test("constructor control")
    ));
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

#[test]
fn macro_generates_typed_fallible_constructors() {
    assert_eq!(
        Ok(7),
        Constructed::construct(vec![SlimValue::String("7".into())]).map(|value| value.0)
    );
    assert_eq!(
        Err(ConstructorError::NoConstructor),
        Constructed::construct(Vec::new())
    );
    assert_eq!(
        Err(ConstructorError::CouldNotInvoke("negative".into())),
        Constructed::construct(vec![SlimValue::String("-1".into())])
    );
    assert_eq!(
        Err(ConstructorError::ArgumentParsingError("i64".into())),
        Constructed::construct(vec![SlimValue::String("not a number".into())])
    );
}

#[test]
fn macro_forwards_missing_methods_to_the_annotated_sut() {
    let mut fixture = SutFixture::construct(Vec::new()).unwrap();
    assert_eq!(
        Ok(SlimValue::String("42".into())),
        fixture.execute_system_under_test("answer", Vec::new())
    );
}

#[test]
fn macro_selects_marked_constructors_by_arity_and_keeps_default_construction() {
    assert_eq!(
        MultiConstructor(0),
        MultiConstructor::construct(Vec::new()).unwrap()
    );
    assert_eq!(
        MultiConstructor(9),
        MultiConstructor::construct(vec![SlimValue::String("9".into())]).unwrap()
    );
    assert_eq!(
        Err(ConstructorError::NoConstructor),
        MultiConstructor::construct(vec![
            SlimValue::String("1".into()),
            SlimValue::String("2".into())
        ])
    );
    assert!(MacroFixture::construct(Vec::new()).is_ok());
    assert!(GenericDefaultFixture::<std::rc::Rc<()>>::construct(Vec::new()).is_ok());
    assert!(BoundedDefaultFixture::<i64>::construct(Vec::new()).is_ok());
}
