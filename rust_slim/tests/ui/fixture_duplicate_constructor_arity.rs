use rust_slim::fixture;

struct Fixture;

#[fixture]
impl Fixture {
    #[slim(constructor)]
    pub fn one(_value: i64) -> Self {
        Self
    }

    #[slim(constructor)]
    pub fn another(_other: String) -> Self {
        Self
    }
}

fn main() {}
