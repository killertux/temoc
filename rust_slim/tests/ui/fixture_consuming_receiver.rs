use rust_slim::fixture;

struct Fixture;

#[fixture]
impl Fixture {
    pub fn consuming(self) {}
}

fn main() {}
