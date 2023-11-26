use rust_slim_macros::fixture;

struct Test {}

#[fixture("Custom.Path.Test")]
impl Test {
    pub fn test_1(&self) {
        todo!()
    }

    pub fn test_2(&self, _a: i64) -> i64 {
        todo!()
    }

    pub fn test_3(_a: i64) -> i64 {
        todo!()
    }

    pub fn test_4(&self, _a: i64, _b: i64) -> i64 {
        todo!()
    }
}

#[test]
fn test() {
    let class = Test {};
    class.test_1();
}
