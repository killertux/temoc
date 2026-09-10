use rust_slim::fixture;

struct Fixture;

#[fixture]
impl Fixture {
    #[slim(constructor)]
    pub fn new() -> i64 {
        42
    }
}

fn main() {}
