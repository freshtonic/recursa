#[test]
fn compiles_unit_terminal() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/unit_terminal.rs");
}

#[test]
fn rejects_label_and_skip() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/trybuild/conflict.rs");
}
