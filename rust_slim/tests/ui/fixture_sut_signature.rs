use rust_slim::fixture;

struct Fixture;

#[fixture]
impl Fixture {
    #[slim(sut)]
    pub fn system_under_test() -> Fixture {
        Fixture
    }
}

fn main() {}
