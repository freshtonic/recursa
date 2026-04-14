#[test]
fn compiles_unit_terminal() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/unit_terminal.rs");
}

#[test]
fn compiles_struct_sequence() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/struct_sequence.rs");
}
