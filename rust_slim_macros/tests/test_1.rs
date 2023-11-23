use rust_slim_macros::fixture;

struct Test{}

#[fixture]
impl Test {
    fn test_1() {
        //
    }

    fn test_2(a: i64) -> i64 {
        //
        todo!()
    }
}

#[test]
fn test() {
    let class = Test{};
    class.test_1();
}