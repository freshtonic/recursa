use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const LOCKFILE: &str = "tests/fixtures/generation/lexing/Cargo.lock";

#[test]
fn every_obsolete_authored_hook_form_has_a_targeted_build_report_diagnostic() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = TempDir::new().expect("temporary obsolete-hook fixtures");
    let target = fixture.path().join("target");
    let cases = cases();

    assert_eq!(
        cases.map(|(_, _, code, _)| code),
        ["RCA3125", "RCA3126", "RCA3117", "RCA3128", "RCA3128"],
        "the obsolete authored-hook contract must retain one stable code per form",
    );

    for (name, source, code, message) in cases {
        let package = fixture.path().join(name);
        fs::create_dir_all(package.join("src")).expect("create obsolete-hook package");
        fs::write(
            package.join("Cargo.toml"),
            format!(
                "[package]\nname = \"recursa-generated-lexing\"\nversion = \"0.0.0\"\nedition = \"2024\"\nrust-version = \"1.88\"\nbuild = \"build.rs\"\n\n[dependencies]\nrecursa = {{ path = {:?} }}\n\n[build-dependencies]\nrecursa-codegen = {{ path = {:?} }}\n\n[workspace]\n",
                workspace.join("recursa"),
                workspace.join("recursa-codegen"),
            ),
        )
        .expect("write obsolete-hook manifest");
        fs::write(
            package.join("build.rs"),
            "fn main() -> Result<(), recursa_codegen::GenerateError> { recursa_codegen::generate(\"src/grammar.rs\")?; Ok(()) }\n",
        )
        .expect("write obsolete-hook build script");
        fs::write(package.join("src/lib.rs"), "pub mod grammar;\n")
            .expect("write obsolete-hook library root");
        fs::write(package.join("src/grammar.rs"), source).expect("write obsolete-hook grammar");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(LOCKFILE),
            package.join("Cargo.lock"),
        )
        .expect("copy locked dependency graph");

        let output = check(&package, &target);
        assert!(
            !output.status.success(),
            "obsolete hook form `{name}` unexpectedly compiled"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("error[{code}] lowering at src/grammar.rs:"))
                && stderr.contains(message),
            "missing targeted diagnostic for `{name}`\nstderr:\n{stderr}",
        );
    }
}

fn cases() -> [(&'static str, &'static str, &'static str, &'static str); 5] {
    [
        (
            "field-callback",
            r#"
recursa::grammar! { module = crate::grammar }

pub fn scan(_: &mut LexContext<'_>) -> LexAction { LexAction::Emit }

#[derive(recursa::Node)]
pub struct Root<'input> {
    #[lex(pattern = r"[a-z]+", callback = crate::grammar::scan)]
    pub word: Word<'input>,
}
"#,
            "RCA3125",
            "authored `#[lex(..., callback = path)]` is unsupported; use a closed matcher",
        ),
        (
            "central-callback",
            r#"
recursa::grammar! { module = crate::grammar }
recursa::tokens! {
    lexer_tokens { Opaque => r"~[a-z]+" with crate::grammar::scan }
}

pub fn scan(_: &mut LexContext<'_>) -> LexAction { LexAction::Emit }

#[derive(recursa::Node)]
#[tok("!")]
pub struct Root;
"#,
            "RCA3126",
            "authored central `with path` lexer callbacks are unsupported; use a closed matcher",
        ),
        (
            "post-lex",
            r#"
recursa::grammar! { module = crate::grammar }
recursa::tokens! { post_lex = crate::grammar::repair }

pub fn repair(_: &mut LexEditor<'_>) {}

#[derive(recursa::Node)]
#[tok("!")]
pub struct Root;
"#,
            "RCA3117",
            "authored `post_lex = path` hooks are unsupported; express tokenization declaratively",
        ),
        (
            "namespaced-parser-postcondition",
            r#"
recursa::grammar! { module = crate::grammar }

#[derive(recursa::Node)]
#[recursa::parser(postcondition = crate::grammar::condition)]
pub struct Root;
"#,
            "RCA3128",
            "authored parser postconditions are unsupported; use a named admission",
        ),
        (
            "parse-postcondition",
            r#"
recursa::grammar! { module = crate::grammar }

#[derive(recursa::Node)]
#[parse(postcondition = crate::grammar::condition)]
pub struct Root;
"#,
            "RCA3128",
            "authored parser postconditions are unsupported; use a named admission",
        ),
    ]
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
