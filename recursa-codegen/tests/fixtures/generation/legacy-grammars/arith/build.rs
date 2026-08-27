fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/arith.rs")?;
    Ok(())
}
