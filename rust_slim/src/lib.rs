//! A SliM V0.5 server implementation for acceptance testing.
//!
//! This was implementating using the documentation found [here](http://fitnesse.org/FitNesse.UserGuide.WritingAcceptanceTests.SliM.SlimProtocol)
//!
//! To add it to your project simply run:
//! ```bash
//! cargo add rust_slim -F macros --dev
//! ```
//!
//! Then you need to create your fixtures. The recomended way of doing this is by using the `#[fixture]` macro. Here is an example:
//!
//! ```rust
//! use rust_slim::fixture;
//!
//! #[derive(Default)]
//! pub struct Calculator {
//!     a: i64,
//!     b: i64,
//! }
//!
//! #[fixture]
//! impl Calculator {
//!     #[slim(constructor)]
//!     pub fn new() -> Self {
//!         Self::default()
//!     }
//!
//!     pub fn set_a(&mut self, a: i64) {
//!         self.a = a
//!     }
//!
//!     pub fn set_b(&mut self, b: i64) {
//!         self.b = b
//!     }
//!
//!     pub fn sum(&self) -> i64 {
//!         self.a + self.b
//!     }
//!
//!     pub fn mul(&self) -> i64 {
//!         self.a * self.b
//!     }
//!
//!     pub fn sub(&self) -> i64 {
//!         self.a - self.b
//!     }
//!
//!     pub fn div(&self) -> i64 {
//!         self.a / self.b
//!     }
//! }
//! ```
//! All methods that are public will be able to be called by the slim server.
//!
//! Than, we need to add an entrypoint to the slim server so we can run it. There are lot of ways of doing this. One is by creating an example in your project.
//!
//! So create and example file called `calculator.rs` and add this:
//! ```rust,no_run
//! use rust_slim::PortSlimServer;
//! use std::env::args;
//!
//! # use anyhow::Result;
//! # use rust_slim::fixture;
//! # #[derive(Default)]
//! # pub struct Calculator;
//! # #[fixture]
//! # impl Calculator {}
//! fn main() -> Result<()> {
//!     let mut server = PortSlimServer::listen_from_args(args().skip(1))?;
//!     server.add_fixture::<Calculator>();
//!     server.run()?;
//!     Ok(())
//! }
//! ```
//! Than, to run it, you simply call `cargo run --example calculator`. Now you need to configure your test runner ([fitnesse](https://fitnesse.org/), [temoc](https://github.com/killertux/temoc/tree/master/temoc)) to call your server

#[cfg(feature = "macros")]
pub use rust_slim_macros::*;
pub use server::{
    OutputTunnel, PortSlimServer, SlimServer, SlimServerError, SlimServerOptions,
    SlimServerOptionsError,
};
use std::fmt::{Display, Formatter};
pub use to_slim_result_string::*;
pub use utils::from_rust_module_path_to_class_path;
#[cfg(feature = "html-hash")]
pub use value::{parse_html_hash, SlimHash};
pub use value::{FromSlimValue, IntoSlimValue, SlimObject, SlimValue};

mod server;
mod to_slim_result_string;
mod utils;
mod value;

/// Fixtures must implement this trait to be able to be executed by the slim server.
/// The `#[fixture]` macro will automatically implement it for the type in the impl block.
pub trait SlimFixture {
    /// Execute a method if it exists in the current fixture.
    /// The `method`is the method name that should be executed.
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError>;

    /// Tries the fixture's System Under Test after a method is not found on
    /// the fixture itself.  Fixtures without an SUT keep the default, which
    /// deliberately looks like a missing method to the dispatcher.
    fn execute_system_under_test(
        &mut self,
        method: &str,
        _args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError> {
        Err(ExecuteMethodError::MethodNotFound {
            method: method.into(),
            class: "SystemUnderTest".into(),
        })
    }
}

/// ClassPath that will be used in the construction of the fixture.
/// It must be pascal case and have its parts separated by a `.`. Eg: `Fixtures.Calculator`
/// The `#[fixture]` macro will automatically implement it for the type in the impl block.
/// By default, the macro will get the current module path and add the fixutre type. For example, a fixutre with the type `Calculator` inside the module `examples::calculator::fixtures` will be converted to a path like `Examples.Calculator.Fixtures.Calculator`.
/// You can use a custom path by passing it to the macro as such
/// ```
/// use rust_slim::fixture;
/// #[derive(Default)]
/// struct Fixture {}
///
/// #[fixture("AnotherPath.MyFixutre")]
/// impl Fixture {}
/// ```
pub trait ClassPath {
    fn class_path() -> String;
}

/// Trait used to construct a fixture from SliM constructor arguments.
///
/// Implementations must report an arity mismatch as [`ConstructorError::NoConstructor`],
/// conversion failures as [`ConstructorError::ArgumentParsingError`], and an
/// error from the constructor itself as [`ConstructorError::CouldNotInvoke`].
pub trait Constructor {
    fn construct(args: Vec<SlimValue>) -> Result<Self, ConstructorError>
    where
        Self: Sized;
}

/// Failure while selecting, converting for, or invoking a fixture constructor.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ConstructorError {
    NoConstructor,
    ArgumentParsingError(String),
    CouldNotInvoke(String),
    /// A constructor asks the runtime to stop or ignore its current batch.
    Control(SlimControlException),
}

/// A structured request from a fixture to change SliM batch execution.
///
/// The reference implementation recognizes exception class names such as
/// `StopTest`. Rust fixtures express the same request by returning this value
/// through [`ExecuteMethodError::Control`].
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SlimControl {
    AbortSlimTest,
    AbortSlimSuite,
    IgnoreScriptTest,
    IgnoreAllTests,
}

impl SlimControl {
    pub fn abort_slim_test(message: impl Into<String>) -> SlimControlException {
        SlimControlException::new(Self::AbortSlimTest, message)
    }

    pub fn abort_slim_suite(message: impl Into<String>) -> SlimControlException {
        SlimControlException::new(Self::AbortSlimSuite, message)
    }

    pub fn ignore_script_test(message: impl Into<String>) -> SlimControlException {
        SlimControlException::new(Self::IgnoreScriptTest, message)
    }

    pub fn ignore_all_tests(message: impl Into<String>) -> SlimControlException {
        SlimControlException::new(Self::IgnoreAllTests, message)
    }
}

/// Details for a fixture-requested SliM batch control result.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SlimControlException {
    pub control: SlimControl,
    pub message: Option<String>,
}

impl SlimControlException {
    pub fn new(control: SlimControl, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            control,
            message: (!message.is_empty()).then_some(message),
        }
    }

    pub fn without_message(control: SlimControl) -> Self {
        Self {
            control,
            message: None,
        }
    }
}

/// Error that can happen while trying to execute a method in a feature.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ExecuteMethodError {
    /// The method might not exists, which should cause a MethodNotFound error.
    MethodNotFound { method: String, class: String },
    /// A fixture argument could not be converted through [`FromSlimValue`].
    ArgumentParsingError(String),
    /// And there might be some failure in the method itself, which should cause an ExecutionError.
    ExecutionError(String),
    /// A fixture asks the SliM runtime to stop or ignore the current work.
    Control(SlimControlException),
}

impl ExecuteMethodError {
    /// Adds the zero-based fixture argument position to a conversion error.
    pub fn for_argument(self, index: usize) -> Self {
        match self {
            Self::ArgumentParsingError(message) => {
                Self::ArgumentParsingError(format!("{}: {message}", index + 1))
            }
            error => error,
        }
    }
}

impl Display for ExecuteMethodError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecuteMethodError::MethodNotFound { method, class } => {
                write!(f, "NO_METHOD_IN_CLASS {method} {class}")
            }
            ExecuteMethodError::ArgumentParsingError(argument) => {
                write!(f, "NO_CONVERTER_FOR_ARGUMENT_NUMBER {argument}")
            }
            ExecuteMethodError::ExecutionError(error) => f.write_str(error),
            ExecuteMethodError::Control(control) => write!(f, "{:?}", control.control),
        }
    }
}
