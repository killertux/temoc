#![cfg(feature = "macros")]

#[test]
fn fixture_macro_reports_unsupported_input_at_compile_time() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/fixture_*.rs");
}
