//! Compile-time contract for a grammar-linked Explorer development launcher.

mod fixture {
    include!("fixtures/generated_launcher.rs");
}

#[test]
fn generated_adapter_satisfies_the_public_launcher_interface() {
    let _: fn() -> Result<(), recursa_explorer::RunError> = fixture::launch;
}
