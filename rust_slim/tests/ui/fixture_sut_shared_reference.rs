use rust_slim::fixture;

#[derive(Default)]
struct Fixture {
    sut: Sut,
}

#[derive(Default)]
struct Sut;

#[fixture]
impl Fixture {
    #[slim(sut)]
    pub fn system_under_test(&self) -> &Sut {
        &self.sut
    }
}

fn main() {}
