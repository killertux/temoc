use anyhow::{bail, Result};
use fixtures::Calculator;
use rust_slim::{ClassPath, SlimFixture};
use std::collections::HashMap;

mod fixtures {
    use super::*;
    pub struct Calculator {
        a: i64,
        b: i64,
    }

    impl Calculator {
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
        fn execute_method(&mut self, method: &str) -> Result<Option<String>> {
            Ok(match method {
                "sum" => Some(format!("{}", self.sum())),
                "mul" => Some(format!("{}", self.mul())),
                "div" => Some(format!("{}", self.div())),
                "sub" => Some(format!("{}", self.sub())),
                _ => bail!("Method not found"),
            })
        }
    }

    impl ClassPath for Calculator {
        fn class_path() -> String {
            format!("{}::Calculator", module_path!(),)
        }
    }

    impl Default for Calculator {
        fn default() -> Self {
            Self { a: 0, b: 0 }
        }
    }
}

struct SlimServer {
    fixtures: HashMap<String, Box<dyn Fn() -> Box<dyn SlimFixture>>>,
}

impl SlimServer {
    pub fn new() -> Self {
        Self {
            fixtures: HashMap::new(),
        }
    }

    pub fn add_fixture<T: ClassPath + Default + SlimFixture + 'static>(&mut self) {
        self.fixtures.insert(
            dbg!(T::class_path()),
            Box::new(|| Box::new(T::default()) as Box<dyn SlimFixture>)
                as Box<dyn Fn() -> Box<dyn SlimFixture>>,
        );
    }
}

fn main() {
    let mut server = SlimServer::new();
    server.add_fixture::<Calculator>();
    println!("Execute the slim server")
}
