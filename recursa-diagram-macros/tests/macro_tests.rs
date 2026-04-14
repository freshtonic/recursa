#[test]
fn compiles_unit_terminal() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/unit_terminal.rs");
}
