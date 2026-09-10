use rust_slim::fixture;

struct Fixture;

#[fixture]
impl Fixture {
    #[slim(constructor)]
    pub fn new(&self) -> Self {
        Self
    }
}

fn main() {}
