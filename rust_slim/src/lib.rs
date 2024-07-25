//! A (not yet complete) implementation of a SlimServer for acceptance testing.
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
//! ```rust
//! use rust_slim::SlimServer;
//! use std::net::TcpListener;
//! use anyhow::Result;
//! use std::env::args;
//! # use std::net::TcpStream;
//! # use std::thread::spawn;
//! # use std::io::Write;
//! # use rust_slim::fixture;
//!
//! # #[derive(Default)]
//! # pub struct Calculator {
//! #     a: i64,
//! #     b: i64,
//! # }
//! #
//! # #[fixture]
//! # impl Calculator {
//! #     pub fn set_a(&mut self, a: i64) {
//! #         self.a = a
//! #     }
//! #
//! #     pub fn set_b(&mut self, b: i64) {
//! #         self.b = b
//! #     }
//! #
//! #     pub fn sum(&self) -> i64 {
//! #         self.a + self.b
//! #     }
//! #
//! #     pub fn mul(&self) -> i64 {
//! #         self.a * self.b
//! #     }
//! #
//! #     pub fn sub(&self) -> i64 {
//! #         self.a - self.b
//! #     }
//! #
//! #     pub fn div(&self) -> i64 {
//! #         self.a / self.b
//! #     }
//! # }
//! #
//! fn main() -> Result<()> {
//! #   spawn(|| {
//! #        loop {
//! #            if let Ok(mut stream) = TcpStream::connect("127.0.0.1:8085") {
//! #                stream.write_all(b"000003:bye").unwrap();
//! #                break;
//! #            }
//! #        }  
//! #    });
//!     let port = args().skip(1).next().unwrap_or("8085".to_string());
//!     let listener = TcpListener::bind(format!("0.0.0.0:{port}").to_string()).expect("Error");
//!     let (stream, _) = listener.accept()?;
//!     let mut server = SlimServer::new(stream.try_clone()?, stream);
//!     server.add_fixture::<Calculator>();
//!     server.run()?;
//!     Ok(())
//! }
//! ```
//! Than, to run it, you simply call `cargo run --example calculator`. Now you need to configure your test runner ([fitnesse](https://fitnesse.org/), [temoc](https://github.com/killertux/temoc/tree/master/temoc)) to call your server

#[cfg(feature = "macros")]
pub use rust_slim_macros::*;
pub use server::SlimServer;
use std::fmt::{Display, Formatter};
pub use to_slim_result_string::*;
pub use utils::from_rust_module_path_to_class_path;

mod server;
mod to_slim_result_string;
mod utils;

/// Fixtures must implement this trait to be able to be executed by the slim server.
/// The `#[fixture]` macro will automatically implement it for the type in the impl block.
pub trait SlimFixture {
    /// Execute a method if it exists in the current fixture.
    /// The `method`is the method name that should be executed.
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<String>,
    ) -> Result<String, ExecuteMethodError>;
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

/// Trait used to construct the fixture. It is auto-implemented for fixtures that also implement Default.
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

/// Error that can happen while trying to execute a method in a feature.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ExecuteMethodError {
    /// The method might not exists, which should cause a MethodNotFound error.
    MethodNotFound { method: String, class: String },
    /// We might have an issue parsing the arguments. The implementation made by the `#[fixture]` macro tries to parse each argument using the [FromStr](https://doc.rust-lang.org/std/str/trait.FromStr.html) trait.
    ArgumentParsingError(String),
    /// And there might be some failure in the method itself, which should cause an ExecutionError.
    ExecutionError(String),
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
        }
    }
}
