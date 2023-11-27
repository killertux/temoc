use anyhow::Result;

#[cfg(feature = "macros")]
pub use rust_slim_macros::*;
pub use server::SlimServer;
pub use to_slim_result_string::*;
pub use utils::from_rust_module_path_to_class_path;

mod server;
mod to_slim_result_string;
mod utils;

pub trait SlimFixture {
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<String>,
    ) -> Result<String, ExecuteMethodError>;
}

pub trait ClassPath {
    fn class_path() -> String;
}

pub trait Constructor {
    fn construct(args: Vec<String>) -> Self;
}

impl<T> Constructor for T
where
    T: Default,
{
    fn construct(_args: Vec<String>) -> T {
        T::default()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExecuteMethodError {
    MethodNotFound { method: String, class: String },
    ArgumentParsingError(String),
    ExecutionError(String),
}

impl ToString for ExecuteMethodError {
    fn to_string(&self) -> String {
        match self {
            ExecuteMethodError::MethodNotFound { method, class } => {
                format!("NO_METHOD_IN_CLASS {method} {class}")
            }
            ExecuteMethodError::ArgumentParsingError(argument) => {
                format!("NO_CONVERTER_FOR_ARGUMENT_NUMBER {argument}")
            }
            ExecuteMethodError::ExecutionError(error) => error.to_string(),
        }
    }
}
