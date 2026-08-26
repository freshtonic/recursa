use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/repetitions";

#[test]
fn a_downstream_crate_honors_capture_off() {
    run_case(CaptureMode::Off);
}

#[test]
fn a_downstream_crate_honors_optional_capture_and_parses_repetitions() {
    run_case(CaptureMode::Optional);
}

#[test]
fn a_downstream_crate_honors_mandatory_capture() {
    run_case(CaptureMode::Mandatory);
}

#[test]
fn diagrams_keep_fixed_attachments_inside_optional_and_repeated_containers() {
    let package = scaffold_fixture(CaptureMode::Optional);
    configure_diagrams(&package);
    let output = run_fixture_doc(&package);
    assert_success(&output);

    let docs = rendered_rustdoc(&package);
    let production = |page: &str| {
        docs.iter()
            .find(|(path, source)| path.ends_with(page) && source.contains("RECURSA_NODE_DIAGRAM"))
            .map(|(_, source)| source.as_str())
            .unwrap_or_else(|| panic!("missing diagram page {page}"))
    };
    let optional = production("grammar/struct.AttachedOptional.html");
    assert!(
        optional.contains("Option&lt;\"(\" grammar::Item \")\"&gt;"),
        "the fixed unit must remain inside Option:\n{optional}",
    );
    let repeated = production("grammar/struct.AttachedMany.html");
    assert!(
        repeated.contains("Vec&lt;\"[\" grammar::Item \"]\"&gt;"),
        "the fixed unit must remain inside Vec:\n{repeated}",
    );
    let nonempty = production("grammar/struct.AttachedNonEmpty.html");
    assert!(
        nonempty.contains("Option&lt;Vec1&lt;\"{\" grammar::Item \"}\"&gt;&gt;"),
        "the fixed unit must remain inside Option<Vec1>:\n{nonempty}",
    );
    for source in [optional, repeated, nonempty] {
        assert!(!source.contains("__recursa_attachment"));
    }
}

#[derive(Clone, Copy)]
enum CaptureMode {
    Off,
    Optional,
    Mandatory,
}

impl CaptureMode {
    const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Optional => "optional",
            Self::Mandatory => "mandatory",
        }
    }
}

fn run_case(mode: CaptureMode) {
    let package = scaffold_fixture(mode);
    let output = run_fixture(&package, mode);

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("repetitions-ok-{}", mode.name())
    );
}

fn scaffold_fixture(mode: CaptureMode) -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    copy(&fixture, package.path(), "build.rs");
    copy(&fixture, package.path(), "Cargo.lock");
    copy(&fixture, package.path(), "src/lib.rs");
    copy(&fixture, package.path(), "src/main.rs");

    let grammar = fs::read_to_string(fixture.join("src/grammar.rs.in"))
        .expect("read generated grammar fixture source")
        .replace("__RECURSA_SPANS_MODE__", mode.name());
    fs::write(package.path().join("src/grammar.rs"), grammar)
        .expect("write generated grammar fixture source");

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generated grammar fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generated grammar fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generated grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir, mode: CaptureMode) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("RECURSA_CAPTURE_MODE", mode.name())
        .env("CARGO_TARGET_DIR", package.path().join("target"));
    command.output().expect("run generated grammar fixture")
}

fn configure_diagrams(package: &TempDir) {
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read repetition grammar");
    fs::write(
        grammar,
        source.replace(
            "    module = crate::grammar,",
            "    module = crate::grammar,\n    diagrams,",
        ),
    )
    .expect("request repetition diagrams");

    let manifest = package.path().join("Cargo.toml");
    let source = fs::read_to_string(&manifest).expect("read repetition manifest");
    let source = source
        .replace(
            "recursa = { path = \"",
            "recursa = { features = [\"diagrams\"], path = \"",
        )
        .replace(
            "recursa-codegen = { path = \"",
            "recursa-codegen = { features = [\"diagrams\"], path = \"",
        );
    fs::write(manifest, source).expect("enable repetition diagram capabilities");
}

fn run_fixture_doc(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("doc")
        .arg("--offline")
        .arg("--quiet")
        .arg("--locked")
        .arg("--no-deps")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("RUSTDOCFLAGS", "-D rustdoc::broken_intra_doc_links")
        .env("CARGO_TARGET_DIR", package.path().join("target"))
        .output()
        .expect("render repetition grammar documentation")
}

fn rendered_rustdoc(package: &TempDir) -> Vec<(PathBuf, String)> {
    let mut rendered = Vec::new();
    collect_rendered_rustdoc(&package.path().join("target/doc"), &mut rendered);
    rendered
}

fn collect_rendered_rustdoc(root: &Path, rendered: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rendered_rustdoc(&path, rendered);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "html")
        {
            let source = fs::read_to_string(&path).expect("read rendered rustdoc page");
            rendered.push((path, source));
        }
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
