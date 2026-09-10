use rust_slim::{
    ClassPath, Constructor, ConstructorError, ExecuteMethodError, OutputTunnel, PortSlimServer,
    SlimControl, SlimFixture, SlimObject, SlimValue,
};
#[cfg(feature = "html-hash")]
use rust_slim::{FromSlimValue, IntoSlimValue, SlimHash};
use std::{env, io::Write};

struct Fixture {
    initial: String,
    sut: Sut,
}

struct Sut;
struct Chained;
struct Actor;
struct LibraryOne;
struct LibraryTwo;

impl ClassPath for Fixture {
    fn class_path() -> String {
        "Conformance.Fixture".into()
    }
}

impl Constructor for Fixture {
    fn construct(args: Vec<SlimValue>) -> Result<Self, ConstructorError> {
        let [SlimValue::String(initial)] = args.as_slice() else {
            return match args.len() {
                1 => Err(ConstructorError::ArgumentParsingError("String".into())),
                _ => Err(ConstructorError::NoConstructor),
            };
        };
        if initial == "fail" {
            return Err(ConstructorError::CouldNotInvoke("requested failure".into()));
        }
        #[cfg(feature = "html-hash")]
        let initial = if initial.starts_with("<table") {
            let hash = SlimHash::from_slim_value(SlimValue::String(initial.clone()))
                .map_err(|error| ConstructorError::ArgumentParsingError(error.to_string()))?;
            format!(
                "hash:{}",
                hash.as_map().get("name").cloned().unwrap_or_default()
            )
        } else {
            initial.clone()
        };
        #[cfg(not(feature = "html-hash"))]
        let initial = initial.clone();
        Ok(Self { initial, sut: Sut })
    }
}

impl SlimFixture for Fixture {
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError> {
        match method {
            "initial" if args.is_empty() => Ok(SlimValue::String(self.initial.clone())),
            "echo" if args.len() == 1 => Ok(args.into_iter().next().expect("one argument checked")),
            "nested" if args.len() == 1 => {
                Ok(args.into_iter().next().expect("one argument checked"))
            }
            "object" if args.is_empty() => {
                Ok(SlimValue::Object(SlimObject::fixture(Chained, "chained")))
            }
            "actor" if args.is_empty() => {
                Ok(SlimValue::Object(SlimObject::fixture(Actor, "actor")))
            }
            "reset" if args.is_empty() => Ok(SlimValue::Void),
            "fail" if args.is_empty() => {
                Err(ExecuteMethodError::ExecutionError("fixture failed".into()))
            }
            "stop" if args.is_empty() => Err(ExecuteMethodError::Control(
                SlimControl::abort_slim_test("stop batch"),
            )),
            "tunnel" if args.is_empty() => {
                let mut stdout = OutputTunnel::stdout(std::io::stderr());
                stdout
                    .write_all(b"first\nsecond\n")
                    .map_err(|error| ExecuteMethodError::ExecutionError(error.to_string()))?;
                stdout
                    .flush()
                    .map_err(|error| ExecuteMethodError::ExecutionError(error.to_string()))?;
                let mut stderr = OutputTunnel::stderr(std::io::stderr());
                stderr
                    .write_all(b"error\ndetail\n")
                    .map_err(|error| ExecuteMethodError::ExecutionError(error.to_string()))?;
                stderr
                    .flush()
                    .map_err(|error| ExecuteMethodError::ExecutionError(error.to_string()))?;
                Ok(SlimValue::Void)
            }
            #[cfg(feature = "html-hash")]
            "hash" if args.len() == 1 => {
                SlimHash::from_slim_value(args.into_iter().next().expect("one argument checked"))
                    .and_then(IntoSlimValue::into_slim_value)
            }
            _ => Err(ExecuteMethodError::MethodNotFound {
                method: method.into(),
                class: Self::class_path(),
            }),
        }
    }

    fn execute_system_under_test(
        &mut self,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError> {
        self.sut.execute_method(method, args)
    }
}

impl SlimFixture for Sut {
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError> {
        match (method, args.is_empty()) {
            ("answer", true) => Ok(SlimValue::String("sut".into())),
            _ => Err(ExecuteMethodError::MethodNotFound {
                method: method.into(),
                class: "Conformance.Sut".into(),
            }),
        }
    }
}

impl SlimFixture for Chained {
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError> {
        if method == "ping" && args.is_empty() {
            Ok(SlimValue::String("chained".into()))
        } else {
            Err(ExecuteMethodError::MethodNotFound {
                method: method.into(),
                class: "Conformance.Chained".into(),
            })
        }
    }
}

impl SlimFixture for Actor {
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError> {
        if method == "name" && args.is_empty() {
            Ok(SlimValue::String("actor".into()))
        } else {
            Err(ExecuteMethodError::MethodNotFound {
                method: method.into(),
                class: "Conformance.Actor".into(),
            })
        }
    }
}

macro_rules! library {
    ($type:ident, $value:literal, $path:literal) => {
        impl ClassPath for $type {
            fn class_path() -> String {
                $path.into()
            }
        }

        impl Constructor for $type {
            fn construct(args: Vec<SlimValue>) -> Result<Self, ConstructorError> {
                if args.is_empty() {
                    Ok(Self)
                } else {
                    Err(ConstructorError::NoConstructor)
                }
            }
        }

        impl SlimFixture for $type {
            fn execute_method(
                &mut self,
                method: &str,
                args: Vec<SlimValue>,
            ) -> Result<SlimValue, ExecuteMethodError> {
                if method == "library_value" && args.is_empty() {
                    Ok(SlimValue::String($value.into()))
                } else {
                    Err(ExecuteMethodError::MethodNotFound {
                        method: method.into(),
                        class: Self::class_path(),
                    })
                }
            }
        }
    };
}

library!(LibraryOne, "one", "Conformance.LibraryOne");
library!(LibraryTwo, "two", "Conformance.LibraryTwo");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut server = PortSlimServer::listen_from_args(env::args().skip(1))?;
    server.add_fixture::<Fixture>();
    server.add_fixture::<LibraryOne>();
    server.add_fixture::<LibraryTwo>();
    server.run()?;
    Ok(())
}
