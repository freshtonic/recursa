fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/json.rs")?;
    Ok(())
}
