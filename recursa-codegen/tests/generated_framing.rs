use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

mod support;

const FIXTURE: &str = "tests/fixtures/generation/framing";

/// Exclusive access to the stable resettable framing workspace.
static FIXTURE_WORKSPACE: OnceLock<Mutex<()>> = OnceLock::new();

/// Monotonic stamp forcing generation after each authored reset.
static FIXTURE_SCENARIO: AtomicU64 = AtomicU64::new(0);

/// Stable downstream package retaining compiled dependency artifacts.
struct Fixture {
    /// Package root below the workspace target directory.
    path: PathBuf,
    /// Serializes authored and generated state changes.
    _guard: MutexGuard<'static, ()>,
}

#[test]
fn generated_framing_partitions_mixed_source_and_rejects_the_first_failing_island() {
    let package = scaffold_fixture();
    let output = run_fixture(&package);
    assert!(
        output.status.success(),
        "framing fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "framing-ok"
    );
}

#[test]
fn framing_references_follow_normal_import_alias_resolution() {
    let package = scaffold_fixture();
    let grammar = package.path.join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read framing grammar");
    fs::write(
        &grammar,
        source
            .replace("island = Entry", "island = FramedEntry")
            .replace(
                "payload(after = Entry::CopyFromStdin",
                "payload(after = FramedEntry::CopyFromStdin",
            )
            + "\ntype EntryAlias<'input> = Entry<'input>;\nuse self::EntryAlias as FramedEntry;\n",
    )
    .expect("author aliased framing references");

    let output = run_fixture(&package);
    assert!(
        output.status.success(),
        "aliased framing references failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn rejected_and_raw_framing_surfaces_have_no_authored_conversion_or_pretty() {
    let package = scaffold_fixture();
    let probes = [
        (
            "rejection AST conversion",
            "into_ast",
            r#"use recursa_generated_framing::grammar;

fn escape(rejection: grammar::FrameRejection<'static>) -> grammar::Entry<'static> {
    rejection.into_ast()
}

fn main() {}
"#,
        ),
        (
            "rejection Pretty rendering",
            "render",
            r#"use recursa::Pretty as _;
use recursa_generated_framing::grammar;

fn escape(rejection: &grammar::FrameRejection<'static>) {
    let _ = rejection.render();
}

fn main() {}
"#,
        ),
        (
            "raw source-region AST conversion",
            "into_ast",
            r#"use recursa_generated_framing::grammar;

fn escape(region: grammar::SourceRegion<'static>) {
    let _ = region.into_ast();
}

fn main() {}
"#,
        ),
        (
            "payload AST conversion",
            "into_ast",
            r#"use recursa_generated_framing::grammar;

fn escape(payload: grammar::PayloadRegion<'static>) {
    let _ = payload.into_ast();
}

fn main() {}
"#,
        ),
    ];
    let main = package.path.join("src/main.rs");
    let mut accessible = Vec::new();
    let mut unrelated = Vec::new();
    for (contract, subject, source) in probes {
        fs::write(&main, source).expect("write framing compile-fail probe");
        let output = check_fixture(&package);
        if output.status.success() {
            accessible.push(contract);
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains(subject) {
                unrelated.push(format!(
                    "{contract} failed without mentioning `{subject}`:\n{stderr}"
                ));
            }
        }
    }
    assert!(
        unrelated.is_empty(),
        "compile-fail probes failed for unrelated reasons:\n{}",
        unrelated.join("\n\n")
    );
    assert!(
        accessible.is_empty(),
        "framing capability boundaries are accessible:\n- {}",
        accessible.join("\n- ")
    );
}

#[test]
fn framing_generation_rejects_an_unreachable_payload_variant() {
    let package = scaffold_fixture();
    let grammar = package.path.join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read framing grammar");
    fs::write(
        &grammar,
        source.replace("Entry::CopyFromStdin", "Entry::Missing"),
    )
    .expect("author unreachable payload transition");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "unreachable payload variant generated"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "payload `after` variant `Missing` is not reachable from framing island `Entry`"
        ),
        "generation failed for an unrelated reason:\n{stderr}"
    );
}

#[test]
fn struct_framing_rejects_an_unassignable_suffix_with_its_strict_diagnostic() {
    let package = scaffold_fixture();
    let grammar = package.path.join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read framing grammar");
    fs::write(
        &grammar,
        source
            .replace("        island = Entry,", "        island = StructEntry,")
            .replace(
                "        payload(after = Entry::CopyFromStdin, terminator = \"\\\\.\")\n",
                "",
            ),
    )
    .expect("select struct framing island");
    fs::write(
        package.path.join("src/main.rs"),
        r#"use recursa_generated_framing::grammar;

fn main() {
    let source = "put 'one' put 'two'";
    let failure = grammar::StructEntry::frame(source)
        .err()
        .expect("a missing struct-island boundary is rejected");
    let grammar::FrameFailure::Rejected(rejection) = failure else {
        panic!("a missing boundary is ordinary invalid input, not a fatal failure")
    };
    assert_eq!(rejection.island(), recursa::Span::new(0, 19).unwrap());
    let framing = rejection
        .framing()
        .expect("the missing boundary names its framing cause");
    assert_eq!(framing.code(), "RCA5002");
    assert_eq!(framing.span(), recursa::Span::new(10, 13).unwrap());
    assert_eq!(rejection.diagnostics().len(), 1);
    assert_eq!(rejection.diagnostics()[0].span(), recursa::Span::new(10, 13).unwrap());
}
"#,
    )
    .expect("author struct framing assertion");

    let output = run_fixture(&package);
    assert!(
        output.status.success(),
        "struct framing fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn scaffold_fixture() -> Fixture {
    let guard = FIXTURE_WORKSPACE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let path = workspace.join("target/generated-fixtures/framing/package");
    let fixture = Fixture {
        path,
        _guard: guard,
    };
    reset_fixture(&fixture.path);
    fixture
}

/// Restores one authored scenario and discards only generated publication state.
fn reset_fixture(package: &Path) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);
    fs::create_dir_all(package).expect("create stable generated framing package");
    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, package, relative);
    }
    let build_script = package.join("build.rs");
    let mut build_source =
        fs::read_to_string(&build_script).expect("read copied framing build script");
    build_source.push_str(&format!(
        "\n// fixture scenario {}\n",
        FIXTURE_SCENARIO.fetch_add(1, Ordering::Relaxed),
    ));
    fs::write(build_script, build_source).expect("stamp framing fixture scenario");
    let lock = fs::read_to_string(manifest_dir.join("tests/fixtures/generation/lexing/Cargo.lock"))
        .expect("read shared generated fixture lockfile")
        .replace("recursa-generated-lexing", "recursa-generated-framing");
    fs::write(package.join("Cargo.lock"), lock).expect("write framing fixture lockfile");
    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read framing fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.join("Cargo.toml"), manifest).expect("write framing fixture manifest");
    support::clean_downstream_package(
        package,
        "recursa-generated-framing",
        &package.join("target"),
    );
}

/// Copies one immutable fixture input into the resettable package.
fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy framing fixture file");
}

/// Renders a local dependency path for a generated TOML manifest.
fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

/// Runs the stable fixture with the workspace Cargo executable.
fn run_fixture(package: &Fixture) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(&package.path)
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", package.path.join("target"))
        .output()
        .expect("run generated framing fixture")
}

/// Type-checks one downstream capability-boundary probe.
fn check_fixture(package: &Fixture) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("check")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(&package.path)
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", package.path.join("target"))
        .output()
        .expect("check generated framing fixture")
}
