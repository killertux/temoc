use rust_slim::fixture;

struct Fixture;

#[fixture]
impl Fixture {
    #[slim(constructor)]
    #[slim(sut)]
    pub fn conflicting() -> Self {
        Self
    }
}

fn main() {}
