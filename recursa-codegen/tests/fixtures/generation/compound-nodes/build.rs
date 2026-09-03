fn main() -> Result<(), recursa_codegen::GenerateError> {
    let generation = recursa_codegen::generate("src/grammar.rs")?;
    let out_dir = std::env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR to build scripts");
    std::fs::write(
        std::path::Path::new(&out_dir).join("recursa-reused"),
        if generation.reused() { "true" } else { "false" },
    )
    .expect("record whether the Generation was reused");
    Ok(())
}
