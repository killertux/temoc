//! A fixture that misbehaves on demand. `temoc/tests/jsonl.rs` runs it.

use rust_slim::{
    ClassPath, Constructor, ConstructorError, ExecuteMethodError, PortSlimServer, SlimControl,
    SlimFixture, SlimValue,
};
use std::env::args;
use std::thread::sleep;
use std::time::Duration;

#[derive(Default)]
struct Probe {
    value: String,
}

impl ClassPath for Probe {
    fn class_path() -> String {
        "Probe".into()
    }
}

impl Constructor for Probe {
    fn construct(_args: Vec<SlimValue>) -> Result<Self, ConstructorError> {
        Ok(Self::default())
    }
}

impl SlimFixture for Probe {
    fn execute_method(
        &mut self,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError> {
        let text = |value: &str| Ok(SlimValue::String(value.to_string()));
        let control = |control| Err(ExecuteMethodError::Control(control));
        match method {
            "set_value" => {
                if let Some(SlimValue::String(value)) = args.into_iter().next() {
                    self.value = value;
                }
                Ok(SlimValue::Void)
            }
            "set_broken" => Err(ExecuteMethodError::ExecutionError("bad setter".into())),
            "value" => text(&self.value),
            "number" => self
                .value
                .parse::<i64>()
                .map(|value| SlimValue::String((value + 1).to_string()))
                .map_err(|error| ExecuteMethodError::ExecutionError(error.to_string())),
            "boom" => Err(ExecuteMethodError::ExecutionError("boom".into())),
            "slow" => {
                sleep(Duration::from_millis(400));
                text("done")
            }
            "hang" => loop {
                sleep(Duration::from_secs(1));
            },
            "die" => std::process::exit(3),
            "noisy" => {
                eprintln!("to stderr");
                text("done")
            }
            "shout" => {
                println!("to stdout");
                text("done")
            }
            "abort_test" => control(SlimControl::abort_slim_test("stop test")),
            "abort_suite" => control(SlimControl::abort_slim_suite("stop suite")),
            "ignore_script" => control(SlimControl::ignore_script_test("skip script")),
            "ignore_all" => control(SlimControl::ignore_all_tests("skip all")),
            "pair" => Ok(SlimValue::List(vec![
                SlimValue::String("left".into()),
                SlimValue::String("right".into()),
            ])),
            _ => Err(ExecuteMethodError::MethodNotFound {
                method: method.into(),
                class: Self::class_path(),
            }),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut server = PortSlimServer::listen_from_args(args().skip(1))?;
    server.add_fixture::<Probe>();
    server.run()?;
    Ok(())
}
