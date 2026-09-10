use rust_slim::fixture;

struct Fixture;

#[fixture]
impl Fixture {
    pub async fn asynchronous(&self) {}
}

fn main() {}
