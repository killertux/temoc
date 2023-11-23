use anyhow::{bail, Result};
use fixtures::Calculator;
use rust_slim::{ClassPath, SlimFixture, SlimServer};
use std::{
    net::TcpListener,
};

mod fixtures {
    use rust_slim::fixture;
    use super::*;

    #[derive(Default)]
    pub struct Calculator {
        a: i64,
        b: i64,
    }

    #[fixture]
    impl Calculator {
        fn set_a(&mut self, a: i64) {
            self.a = a
        }

        fn set_b(&mut self, b: i64) {
            self.b = b
        }

        fn sum(&self) -> i64 {
            self.a + self.b
        }

        fn mul(&self) -> i64 {
            self.a * self.b
        }

        fn sub(&self) -> i64 {
            self.a - self.b
        }

        fn div(&self) -> i64 {
            self.a / self.b
        }
    }

    impl SlimFixture for Calculator {
        fn execute_method(&mut self, method: &str, args: Vec<String>) -> Result<Option<String>> {
            Ok(match method {
                "sum" => Some(format!("{}", self.sum())),
                "mul" => Some(format!("{}", self.mul())),
                "div" => Some(format!("{}", self.div())),
                "sub" => Some(format!("{}", self.sub())),
                "set_a" => {
                    self.set_a(args[0].parse()?);
                    None
                }
                "set_b" => {
                    self.set_b(args[0].parse()?);
                    None
                }
                _ => bail!("Method not found"),
            })
        }
    }

    impl ClassPath for Calculator {
        fn class_path() -> String {
            format!("{}::Calculator", module_path!(),)
        }
    }
}

fn main() -> Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:8426"))?;
    let (stream, _) = listener.accept()?;
    let mut server = SlimServer::new(stream.try_clone()?, stream);
    server.add_fixture::<Calculator>();
    server.run()?;
    Ok(())
}
