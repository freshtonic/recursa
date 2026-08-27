/// Representative development-binary entry using a generated root adapter.
pub fn launch() -> Result<(), recursa_explorer::RunError> {
    recursa_explorer::run(recursa_example_json::grammar::Value::explorer())
}
