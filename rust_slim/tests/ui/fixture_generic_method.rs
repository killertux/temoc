use rust_slim::fixture;

struct Fixture;

#[fixture]
impl Fixture {
    pub fn generic<T>(&self, _value: T) {}
}

fn main() {}
