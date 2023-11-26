use anyhow::Result;
use fixtures::Calculator;
use rust_slim::SlimServer;
use std::net::TcpListener;

mod fixtures {
    use rust_slim::fixture;

    #[derive(Default)]
    pub struct Calculator {
        a: i64,
        b: i64,
    }

    #[fixture]
    impl Calculator {
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
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args();
    args.next();
    let Some(port) = args.next() else {
        println!("No port provided");
        return Ok(());
    };
    let listener = TcpListener::bind(format!("0.0.0.0:{port}").to_string())?;
    let (stream, _) = listener.accept()?;
    let mut server = SlimServer::new(stream.try_clone()?, stream);
    server.add_fixture::<Calculator>();
    server.run()?;
    Ok(())
}
