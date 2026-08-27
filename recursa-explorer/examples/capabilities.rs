//! Checkout-only launcher for manually smoking optional Explorer capabilities.

fn main() -> Result<(), recursa_explorer::RunError> {
    recursa_explorer::run(recursa_explorer_capabilities_fixture::grammar::Root::explorer())
}
