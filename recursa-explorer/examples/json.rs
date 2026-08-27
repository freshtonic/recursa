//! Checkout-only launcher for manually smoking the bundled Explorer UI.

fn main() -> Result<(), recursa_explorer::RunError> {
    recursa_explorer::run(recursa_example_json::grammar::Value::explorer())
}
