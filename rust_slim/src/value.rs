//! Values passed between the SliM executor and Rust fixtures.
//!
//! `SlimValue` deliberately differs from `slim_protocol::SlimValue`: the
//! protocol only has strings and lists, while fixture execution also needs
//! null, void, and values that stay in-process as object symbols.

use crate::{ExecuteMethodError, SlimControlException};
use chrono::NaiveDate;
use std::{any::Any, cell::RefCell, fmt, rc::Rc};

enum SlimObjectValue {
    Opaque(Rc<dyn Any>),
    Fixture(Rc<RefCell<dyn crate::SlimFixture>>),
}

impl Clone for SlimObjectValue {
    fn clone(&self) -> Self {
        match self {
            Self::Opaque(value) => Self::Opaque(Rc::clone(value)),
            Self::Fixture(value) => Self::Fixture(Rc::clone(value)),
        }
    }
}

/// An opaque, cloneable object kept by a single-threaded SliM server.
///
/// The handle uses `Rc`, rather than `Arc`, because fixtures are executed by
/// one server thread. `display` is used only when an object must temporarily
/// be represented as text; future symbol dispatch can retain the handle.
#[derive(Clone)]
pub struct SlimObject {
    value: SlimObjectValue,
    display: String,
}

impl SlimObject {
    pub fn new<T: Any>(value: T, display: impl Into<String>) -> Self {
        Self {
            value: SlimObjectValue::Opaque(Rc::new(value)),
            display: display.into(),
        }
    }

    /// Creates an object that can later be copied into the server's fixture
    /// registry by the SliM `make` fixture-chaining behavior.
    pub fn fixture<T: crate::SlimFixture + 'static>(value: T, display: impl Into<String>) -> Self {
        Self {
            value: SlimObjectValue::Fixture(Rc::new(RefCell::new(value))),
            display: display.into(),
        }
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        match &self.value {
            SlimObjectValue::Opaque(value) => value.downcast_ref(),
            SlimObjectValue::Fixture(_) => None,
        }
    }

    pub fn downcast<T: Any>(self) -> Result<Rc<T>, Self> {
        let Self { value, display } = self;
        match value {
            SlimObjectValue::Opaque(value) => value.downcast::<T>().map_err(|value| Self {
                value: SlimObjectValue::Opaque(value),
                display,
            }),
            value @ SlimObjectValue::Fixture(_) => Err(Self { value, display }),
        }
    }

    pub fn as_fixture(&self) -> Option<Rc<RefCell<dyn crate::SlimFixture>>> {
        match &self.value {
            SlimObjectValue::Opaque(_) => None,
            SlimObjectValue::Fixture(value) => Some(Rc::clone(value)),
        }
    }

    pub fn display_value(&self) -> &str {
        &self.display
    }
}

impl fmt::Debug for SlimObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SlimObject").field(&self.display).finish()
    }
}

/// The typed value model exposed to fixture implementations.
#[derive(Clone, Debug)]
pub enum SlimValue {
    String(String),
    List(Vec<SlimValue>),
    Null,
    Void,
    Object(SlimObject),
}

impl PartialEq for SlimValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(left), Self::String(right)) => left == right,
            (Self::List(left), Self::List(right)) => left == right,
            (Self::Null, Self::Null) | (Self::Void, Self::Void) => true,
            (Self::Object(left), Self::Object(right)) => match (&left.value, &right.value) {
                (SlimObjectValue::Opaque(left), SlimObjectValue::Opaque(right)) => {
                    Rc::ptr_eq(left, right)
                }
                (SlimObjectValue::Fixture(left), SlimObjectValue::Fixture(right)) => {
                    Rc::ptr_eq(left, right)
                }
                _ => false,
            },
            _ => false,
        }
    }
}

impl Eq for SlimValue {}

impl From<String> for SlimValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for SlimValue {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}

impl SlimValue {
    pub fn object<T: Any>(value: T, display: impl Into<String>) -> Self {
        Self::Object(SlimObject::new(value, display))
    }

    /// Lossy textual form used by the compatibility symbol replacement path.
    pub fn as_text(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::List(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Self::as_text)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Null => "null".into(),
            Self::Void => String::new(),
            Self::Object(value) => value.display_value().into(),
        }
    }
}

/// Converts one fixture argument from its runtime representation.
///
/// Implement this trait for application-specific argument types to use them
/// in a `#[fixture]` method.
pub trait FromSlimValue: Sized {
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError>;
}

/// Converts a fixture return value into a runtime value.
///
/// Application types can implement this trait directly without depending on
/// the wire protocol representation.
pub trait IntoSlimValue {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError>;
}

impl FromSlimValue for SlimValue {
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
        Ok(value)
    }
}

impl IntoSlimValue for SlimValue {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        Ok(self)
    }
}

impl IntoSlimValue for SlimControlException {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        Err(ExecuteMethodError::Control(self))
    }
}

impl FromSlimValue for SlimObject {
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
        match value {
            SlimValue::Object(value) => Ok(value),
            other => Err(conversion_error("object", &other)),
        }
    }
}

impl IntoSlimValue for SlimObject {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        Ok(SlimValue::Object(self))
    }
}

impl FromSlimValue for String {
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
        match value {
            SlimValue::String(value) => Ok(value),
            other => Err(conversion_error("String", &other)),
        }
    }
}

impl IntoSlimValue for String {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        Ok(SlimValue::String(self))
    }
}

impl IntoSlimValue for &str {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        Ok(SlimValue::String(self.into()))
    }
}

macro_rules! scalar_values {
    ($($ty:ty),* $(,)?) => {$(
        impl FromSlimValue for $ty {
            fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
                let SlimValue::String(value) = value else {
                    return Err(ExecuteMethodError::ArgumentParsingError(
                        format!("expected a string for {}", stringify!($ty)),
                    ));
                };
                value.parse::<$ty>().map_err(|error| {
                    ExecuteMethodError::ArgumentParsingError(error.to_string())
                })
            }
        }

        impl IntoSlimValue for $ty {
            fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
                Ok(SlimValue::String(self.to_string()))
            }
        }
    )*};
}

scalar_values!(bool, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64);

impl FromSlimValue for NaiveDate {
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
        let SlimValue::String(value) = value else {
            return Err(ExecuteMethodError::ArgumentParsingError(
                "expected a date string in dd-MMM-yyyy form".into(),
            ));
        };
        Self::parse_from_str(&value, "%d-%b-%Y")
            .map_err(|error| ExecuteMethodError::ArgumentParsingError(error.to_string()))
    }
}

impl IntoSlimValue for NaiveDate {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        Ok(SlimValue::String(self.format("%d-%b-%Y").to_string()))
    }
}

impl<T> FromSlimValue for Option<T>
where
    T: FromSlimValue,
{
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
        match value {
            SlimValue::Null => Ok(None),
            value => T::from_slim_value(value).map(Some),
        }
    }
}

impl<T> IntoSlimValue for Option<T>
where
    T: IntoSlimValue,
{
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        match self {
            Some(value) => value.into_slim_value(),
            None => Ok(SlimValue::Null),
        }
    }
}

impl<T> FromSlimValue for Vec<T>
where
    T: FromSlimValue,
{
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
        let SlimValue::List(values) = value else {
            return Err(ExecuteMethodError::ArgumentParsingError(
                "expected a list".into(),
            ));
        };
        values.into_iter().map(T::from_slim_value).collect()
    }
}

impl<T> IntoSlimValue for Vec<T>
where
    T: IntoSlimValue,
{
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        self.into_iter()
            .map(T::into_slim_value)
            .collect::<Result<Vec<_>, _>>()
            .map(SlimValue::List)
    }
}

impl<T, const N: usize> FromSlimValue for [T; N]
where
    T: FromSlimValue,
{
    fn from_slim_value(value: SlimValue) -> Result<Self, ExecuteMethodError> {
        let values = Vec::<T>::from_slim_value(value)?;
        values.try_into().map_err(|values: Vec<T>| {
            ExecuteMethodError::ArgumentParsingError(format!(
                "expected a list with {N} items, got {}",
                values.len()
            ))
        })
    }
}

impl<T, const N: usize> IntoSlimValue for [T; N]
where
    T: IntoSlimValue,
{
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        self.into_iter().collect::<Vec<_>>().into_slim_value()
    }
}

impl IntoSlimValue for () {
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        Ok(SlimValue::Void)
    }
}

impl<T, E> IntoSlimValue for Result<T, E>
where
    T: IntoSlimValue,
    E: fmt::Display,
{
    fn into_slim_value(self) -> Result<SlimValue, ExecuteMethodError> {
        match self {
            Ok(value) => value.into_slim_value(),
            Err(error) => Err(ExecuteMethodError::ExecutionError(error.to_string())),
        }
    }
}

fn conversion_error(expected: &str, actual: &SlimValue) -> ExecuteMethodError {
    ExecuteMethodError::ArgumentParsingError(format!("expected {expected}, got {actual:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ObjectFixture;

    impl crate::SlimFixture for ObjectFixture {
        fn execute_method(
            &mut self,
            method: &str,
            _args: Vec<SlimValue>,
        ) -> Result<SlimValue, ExecuteMethodError> {
            Ok(SlimValue::String(method.into()))
        }
    }

    #[test]
    fn nested_lists_round_trip_through_conversions() {
        let value = vec![vec![1_i64, 2], vec![3]].into_slim_value().unwrap();
        assert!(matches!(value, SlimValue::List(_)));
        assert_eq!(
            vec![vec![1_i64, 2], vec![3]],
            Vec::<Vec<i64>>::from_slim_value(value).unwrap()
        );
    }

    #[test]
    fn null_and_void_are_distinct() {
        assert!(matches!(
            Option::<String>::None.into_slim_value().unwrap(),
            SlimValue::Null
        ));
        assert!(matches!(().into_slim_value().unwrap(), SlimValue::Void));
        assert_eq!(
            None,
            Option::<String>::from_slim_value(SlimValue::Null).unwrap()
        );
    }

    #[test]
    fn invalid_primitive_is_a_conversion_error() {
        assert!(matches!(
            i64::from_slim_value(SlimValue::String("not a number".into())),
            Err(ExecuteMethodError::ArgumentParsingError(_))
        ));
    }

    #[test]
    fn dates_use_the_documented_slim_format() {
        let date = NaiveDate::from_ymd_opt(1970, 10, 10).unwrap();
        let value = date.into_slim_value().unwrap();
        assert_eq!(SlimValue::String("10-Oct-1970".into()), value);
        assert_eq!(date, NaiveDate::from_slim_value(value).unwrap());
    }

    #[test]
    fn lists_and_objects_have_useful_runtime_conversions() {
        let list = SlimValue::List(vec![
            SlimValue::String("one".into()),
            SlimValue::List(vec![SlimValue::String("two".into())]),
        ]);
        assert_eq!("[one, [two]]", list.as_text());

        let object = SlimObject::new(42_i64, "the answer");
        let value = object.clone().into_slim_value().unwrap();
        assert_eq!(
            42,
            *SlimObject::from_slim_value(value)
                .unwrap()
                .downcast::<i64>()
                .unwrap()
        );

        let fixture = SlimObject::fixture(ObjectFixture, "fixture");
        let callable = fixture.as_fixture().unwrap();
        assert_eq!(
            Ok(SlimValue::String("called".into())),
            callable.borrow_mut().execute_method("called", Vec::new())
        );
    }

    #[test]
    fn result_errors_are_execution_errors() {
        let value: Result<i64, &str> = Err("fixture failed");
        assert_eq!(
            Err(ExecuteMethodError::ExecutionError("fixture failed".into())),
            value.into_slim_value()
        );
    }
}
