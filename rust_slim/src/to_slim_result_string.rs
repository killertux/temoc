use crate::ExecuteMethodError;

/// Converts the result of a method into a result that the SlimServer can handle. This is mainly used so you can return whatever you want in a method and we can convert it inside the macro expansion of the `[fixture]` macro. If you are implementating the [SlimFixture](crate::SlimFixture) manually, you can ignore this.
/// It has implementations for most basic types.
pub trait ToSlimResultString {
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError>;
}

macro_rules! impl_to_slim_result_string {
    ($t:ident) => {
        impl ToSlimResultString for $t {
            fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
                Ok(self.to_string())
            }
        }
    };
}

impl_to_slim_result_string!(u8);
impl_to_slim_result_string!(u16);
impl_to_slim_result_string!(u32);
impl_to_slim_result_string!(u64);
impl_to_slim_result_string!(usize);
impl_to_slim_result_string!(i8);
impl_to_slim_result_string!(i16);
impl_to_slim_result_string!(i32);
impl_to_slim_result_string!(i64);
impl_to_slim_result_string!(isize);
impl_to_slim_result_string!(f32);
impl_to_slim_result_string!(f64);
impl_to_slim_result_string!(String);
impl_to_slim_result_string!(bool);

impl ToSlimResultString for () {
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        Ok(String::from("/__VOID__/"))
    }
}

impl<'a> ToSlimResultString for &'a str {
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        Ok(self.to_string())
    }
}

impl<T> ToSlimResultString for Option<T>
where
    T: ToString,
{
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        match self {
            None => Ok(String::from("null")),
            Some(value) => Ok(value.to_string()),
        }
    }
}

impl<T, E> ToSlimResultString for Result<T, E>
where
    T: ToString,
    E: ToString,
{
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        match self {
            Err(e) => Err(ExecuteMethodError::ExecutionError(e.to_string())),
            Ok(value) => Ok(value.to_string()),
        }
    }
}

impl<T, const S: usize> ToSlimResultString for [T; S]
where
    T: ToSlimResultString,
{
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        iterator_to_slim_result_string(self.into_iter())
    }
}

impl<T> ToSlimResultString for Vec<T>
where
    T: ToSlimResultString,
{
    fn to_slim_result_string(self) -> Result<String, ExecuteMethodError> {
        iterator_to_slim_result_string(self.into_iter())
    }
}

fn iterator_to_slim_result_string(
    iterator: impl Iterator<Item = impl ToSlimResultString>,
) -> Result<String, ExecuteMethodError> {
    Ok(format!(
        "/__ARRAY[{}]__/",
        iterator
            .map(|v| v.to_slim_result_string())
            .collect::<Result<Vec<String>, ExecuteMethodError>>()?
            .join("__|__")
    ))
}
