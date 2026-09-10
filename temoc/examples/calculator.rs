use anyhow::Result;
use fixtures::CalculatorFixture;
use rust_slim::PortSlimServer;
use std::env::args;

mod fixtures {
    use rust_slim::fixture;

    #[derive(Default)]
    pub struct CalculatorFixture {
        a: i64,
        b: i64,
    }

    #[fixture]
    impl CalculatorFixture {
        #[slim(constructor)]
        pub fn new() -> Self {
            Self::default()
        }

        pub fn set_a(&mut self, a: i64) {
            self.a = a
        }

        pub fn set_b(&mut self, b: i64) {
            self.b = b
        }

        pub fn sum(&self) -> i64 {
            self.a + self.b
        }

        pub fn mul(&self) -> i64 {
            self.a * self.b
        }

        pub fn sub(&self) -> i64 {
            self.a - self.b
        }

        pub fn div(&self) -> i64 {
            self.a / self.b
        }

        pub fn log(&self, a: f64, b: f64) -> [String; 2] {
            [format!("{:.2}", a.log(b)), format!("{:.2}", b.log(a))]
        }
    }
}

fn main() -> Result<()> {
    let mut server = PortSlimServer::listen_from_args(args().skip(1))?;

    server.add_fixture::<CalculatorFixture>();
    server.run()?;
    Ok(())
}
