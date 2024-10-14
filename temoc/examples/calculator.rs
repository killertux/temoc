use anyhow::Result;
use fixtures::CalculatorFixture;
use rust_slim::SlimServer;
use std::env::args;
use std::io::{stdin, stdout, Read, Write};
use std::net::TcpListener;

mod fixtures {
    use rust_slim::fixture;

    #[derive(Default)]
    pub struct CalculatorFixture {
        a: i64,
        b: i64,
    }

    #[fixture]
    impl CalculatorFixture {
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
    let port = args().skip(1).next().unwrap_or("8085".to_string());
    let mut server = build_server(&port)?;

    server.add_fixture::<CalculatorFixture>();
    server.run()?;
    Ok(())
}

fn build_server(port: &str) -> Result<SlimServer<Box<dyn Read>, Box<dyn Write>>> {
    Ok(if port == "1" {
        SlimServer::new(Box::new(stdin()), Box::new(stdout()))
    } else {
        let listener = TcpListener::bind(format!("0.0.0.0:{port}").to_string())?;
        let (stream, _) = listener.accept()?;
        SlimServer::new(Box::new(stream.try_clone()?), Box::new(stream))
    })
}
