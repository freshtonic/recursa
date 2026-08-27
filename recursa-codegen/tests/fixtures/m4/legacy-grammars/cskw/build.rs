fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/cskw.rs")?;
    Ok(())
}
