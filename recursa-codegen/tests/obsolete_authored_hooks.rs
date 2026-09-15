use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const LOCKFILE: &str = "tests/fixtures/generation/lexing/Cargo.lock";

#[test]
fn downstream_cargo_smokes_one_obsolete_hook_diagnostic() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let package = TempDir::new().expect("temporary obsolete-hook fixture");
    let target = package.path().join("target");
    fs::create_dir_all(package.path().join("src")).expect("create obsolete-hook package");
    fs::write(
        package.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"recursa-generated-lexing\"\nversion = \"0.0.0\"\nedition = \"2024\"\nrust-version = \"1.88\"\nbuild = \"build.rs\"\n\n[dependencies]\nrecursa = {{ path = {:?} }}\n\n[build-dependencies]\nrecursa-codegen = {{ path = {:?} }}\n\n[workspace]\n",
            workspace.join("recursa"),
            workspace.join("recursa-codegen"),
        ),
    )
    .expect("write obsolete-hook manifest");
    fs::write(
        package.path().join("build.rs"),
        "fn main() -> Result<(), recursa_codegen::GenerateError> { recursa_codegen::generate(\"src/grammar.rs\")?; Ok(()) }\n",
    )
    .expect("write obsolete-hook build script");
    fs::write(package.path().join("src/lib.rs"), "pub mod grammar;\n")
        .expect("write obsolete-hook library root");
    fs::write(
        package.path().join("src/grammar.rs"),
        r#"recursa::grammar! { module = crate::grammar }

pub fn scan(_: &mut LexContext<'_>) -> LexAction {
    LexAction::Emit
}

recursa::ast_node! {

    pub struct Root {
        #[lex(pattern = r"[a-z]+", callback = crate::grammar::scan)]
        pub word: Word,
    }
}
"#,
    )
    .expect("write obsolete-hook grammar");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(LOCKFILE),
        package.path().join("Cargo.lock"),
    )
    .expect("copy locked dependency graph");

    let output = check(package.path(), &target);
    assert!(
        !output.status.success(),
        "obsolete authored hook unexpectedly compiled"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error[RCA3125] lowering at src/grammar.rs:")
            && stderr.contains(
                "authored `#[lex(..., callback = path)]` is unsupported; use a closed matcher",
            ),
        "missing targeted downstream diagnostic\nstderr:\n{stderr}",
    );
}

fn check(package: &Path, target: &Path) -> std::process::Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("check")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package)
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target)
        .output()
        .expect("check obsolete-hook fixture")
}
