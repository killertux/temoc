use crate::ExecuteMethodError;

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
