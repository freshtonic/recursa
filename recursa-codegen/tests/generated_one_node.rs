use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/one-node";

#[test]
fn a_downstream_crate_loads_and_parses_one_generated_node_through_the_facade() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let main = package.path().join("src/main.rs");
    let valid_source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    let valid_main = fs::read_to_string(&main).expect("read scaffolded downstream consumer");

    let initial = run_fixture(&package);

    assert_success(&initial);
    let selected = find_selected_generation(&target_dir(&package));
    assert_eq!(
        String::from_utf8(initial.stdout).unwrap().trim(),
        "one-node-ok"
    );

    fs::write(
        &grammar,
        "recursa::grammar! { module = crate::grammar }\n\npub struct Broken {\n",
    )
    .expect("replace the selected grammar with invalid Rust");
    let failed = run_fixture(&package);
    assert!(
        !failed.status.success(),
        "a hard generation failure must stop the Cargo build"
    );
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("RCA2001"),
        "the invalid selected grammar must fail in generation, not only in downstream compilation"
    );
    assert!(
        !String::from_utf8_lossy(&failed.stdout).contains("one-node-ok"),
        "Cargo must not run the stale previously built binary after generation fails"
    );
    assert!(
        !selected.exists(),
        "a failed generation must invalidate the previous selector"
    );

    let revised_source = valid_source
        .replace("Word", "Name")
        .replace("r#match", "r#loop");
    let revised_main = valid_main
        .replace("Word", "Name")
        .replace("r#match", "r#loop");
    assert_ne!(revised_source, valid_source);
    assert_ne!(revised_main, valid_main);
    fs::write(&grammar, revised_source).expect("replace the failure with a revised valid grammar");
    fs::write(&main, revised_main).expect("use the revised generated grammar API");
    let restored = run_fixture(&package);
    assert_success(&restored);
    assert_eq!(
        String::from_utf8(restored.stdout).unwrap().trim(),
        "one-node-ok"
    );
}

#[test]
fn a_node_body_rejects_the_same_semantic_key_from_a_different_physical_declaration() {
    let package = scaffold_fixture();
    fs::copy(
        package.path().join("src/grammar.rs"),
        package.path().join("src/analyzed.rs"),
    )
    .expect("copy the grammar to a physically distinct analyzed source");
    fs::write(
        package.path().join("build.rs"),
        r#"fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/analyzed.rs")?;
    Ok(())
}
"#,
    )
    .expect("select the replacement declaration for analysis");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "a replacement physical declaration must not load the compiled Node body"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("RCA9002"),
        "the declaration-local anchor mismatch must fail at the Node loader\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn structural_loaders_report_missing_output_without_cascades_or_fallbacks() {
    let package = scaffold_fixture();
    assert_success(&run_fixture(&package));

    let selected = find_selected_generation(&target_dir(&package));
    let selector = fs::read_to_string(&selected).expect("read selected Generation");
    let identity = selector
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// recursa-generation:"))
        .expect("selector carries its private Generation identity");
    let generation = selected
        .parent()
        .unwrap()
        .join("recursa-generations")
        .join(identity);
    let root = generation.join("root.rs");
    let node = generation.join("node-0.rs");
    let root_rust = fs::read(&root).expect("read generated root");
    let node_rust = fs::read(&node).expect("read generated Node body");

    fs::remove_file(&root).expect("remove generated root");
    force_consumer_recompile(&package, "root-absence");
    let missing_root = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&missing_root.stderr);
    assert!(!missing_root.status.success(), "missing root must fail");
    assert!(stderr.contains("RCA9001"), "stderr:\n{stderr}");
    assert!(!stderr.contains("RCA9002"), "stderr:\n{stderr}");

    fs::write(&root, &root_rust).expect("restore generated root");
    fs::remove_file(&node).expect("remove generated Node body");
    force_consumer_recompile(&package, "node-absence");
    let missing_node = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&missing_node.stderr);
    assert!(
        !missing_node.status.success(),
        "missing Node body must fail"
    );
    assert!(stderr.contains("RCA9002"), "stderr:\n{stderr}");
    assert!(!stderr.contains("RCA9001"), "stderr:\n{stderr}");

    fs::write(&node, &node_rust).expect("restore generated Node body");
    let incompatible_root = String::from_utf8(root_rust.clone())
        .unwrap()
        .replace("protocol_1", "protocol_999");
    assert_ne!(incompatible_root.as_bytes(), root_rust);
    fs::write(&root, incompatible_root).unwrap();
    force_consumer_recompile(&package, "protocol-mismatch");
    let mismatch = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&mismatch.stderr);
    assert!(!mismatch.status.success(), "protocol mismatch must fail");
    assert_eq!(
        stderr.matches("error: error[RCA9003]").count(),
        1,
        "stderr:\n{stderr}"
    );
    assert!(!stderr.contains("RCA9002"), "stderr:\n{stderr}");

    fs::write(&root, &root_rust).expect("restore protocol-compatible root");
    let conflicting_node = String::from_utf8(node_rust.clone())
        .unwrap()
        .replace(identity, "g_0000000000000000");
    assert_ne!(conflicting_node.as_bytes(), node_rust);
    fs::write(&node, conflicting_node).unwrap();
    force_consumer_recompile(&package, "generation-conflict");
    let conflict = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&conflict.stderr);
    assert!(!conflict.status.success(), "Generation conflict must fail");
    assert!(stderr.contains("RCA9004"), "stderr:\n{stderr}");

    fs::write(&node, &node_rust).expect("restore matching Node body");
    fs::write(&node, "this is externally malformed generated Rust").unwrap();
    force_consumer_recompile(&package, "malformed-node");
    let malformed = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&malformed.stderr);
    assert!(
        !malformed.status.success(),
        "malformed generated Node Rust must fail"
    );
    assert!(
        stderr.contains("expected one of") || stderr.contains("expected `!`"),
        "native Rust syntax diagnostics must remain authoritative\nstderr:\n{stderr}"
    );
    assert!(!stderr.contains("RCA9002"), "stderr:\n{stderr}");

    fs::write(&node, &node_rust).expect("restore generated Node body after corruption");
    fs::write(&root, "this is externally malformed generated Rust").unwrap();
    force_consumer_recompile(&package, "malformed-root");
    let malformed = run_fixture(&package);
    let stderr = String::from_utf8_lossy(&malformed.stderr);
    assert!(
        !malformed.status.success(),
        "malformed generated Rust must fail"
    );
    assert!(
        stderr.contains("expected one of") || stderr.contains("expected `!`"),
        "native Rust syntax diagnostics must remain authoritative\nstderr:\n{stderr}"
    );
    assert!(!stderr.contains("RCA9001"), "stderr:\n{stderr}");
    assert!(!stderr.contains("RCA9002"), "stderr:\n{stderr}");
    assert!(!stderr.contains("runtime fallback"), "stderr:\n{stderr}");

    fs::write(&root, root_rust).expect("restore generated root after corruption");
    force_consumer_recompile(&package, "clean-recovery");
    assert_success(&run_fixture(&package));
}

#[test]
fn debug_directory_changes_rerun_generation_without_changing_compiled_output() {
    let package = scaffold_fixture();
    assert_success(&run_fixture(&package));
    let selected = find_selected_generation(&target_dir(&package));
    let selector = fs::read_to_string(&selected).unwrap();
    let identity = selector
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// recursa-generation:"))
        .unwrap()
        .to_owned();
    let generation = selected
        .parent()
        .unwrap()
        .join("recursa-generations")
        .join(&identity);
    let root = fs::read(generation.join("root.rs")).unwrap();
    let node = fs::read(generation.join("node-0.rs")).unwrap();

    let first_destination = tempfile::tempdir().unwrap();
    assert_success(&run_fixture_with_debug(&package, first_destination.path()));
    assert_eq!(fs::read_to_string(&selected).unwrap(), selector);
    assert_eq!(fs::read(generation.join("root.rs")).unwrap(), root);
    assert_eq!(fs::read(generation.join("node-0.rs")).unwrap(), node);
    let first_index = find_one_debug_index(first_destination.path());

    let second_destination = tempfile::tempdir().unwrap();
    assert_success(&run_fixture_with_debug(&package, second_destination.path()));
    assert_eq!(fs::read_to_string(&selected).unwrap(), selector);
    let second_index = find_one_debug_index(second_destination.path());
    assert_eq!(
        fs::read_to_string(&first_index).unwrap(),
        fs::read_to_string(&second_index).unwrap(),
        "the destination is not part of deterministic debug content"
    );

    let bundle = first_index.parent().unwrap();
    fs::write(bundle.join("root.rs"), "human inspection notes").unwrap();
    fs::write(bundle.join("extra-notes.txt"), "temporary notes").unwrap();
    assert_success(&run_fixture_with_debug(&package, first_destination.path()));
    assert_eq!(fs::read_to_string(&selected).unwrap(), selector);
    assert!(!bundle.join("extra-notes.txt").exists());
    syn::parse_file(&fs::read_to_string(bundle.join("root.rs")).unwrap())
        .expect("a rerun replaces human edits instead of loading them");

    for line in fs::read_to_string(&first_index).unwrap().lines().skip(2) {
        let private_name = line.split('\t').nth(2).expect("indexed private filename");
        syn::parse_file(&fs::read_to_string(bundle.join(private_name)).unwrap())
            .expect("every indexed logical output is parseable Rust");
    }
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    copy(&fixture, package.path(), "build.rs");
    copy(&fixture, package.path(), "Cargo.lock");
    copy(&fixture, package.path(), "src/lib.rs");
    copy(&fixture, package.path(), "src/grammar.rs");
    copy(&fixture, package.path(), "src/main.rs");

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

fn run_fixture(package: &TempDir) -> Output {
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
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("run generated grammar fixture")
}

fn run_fixture_with_debug(package: &TempDir, debug_directory: &Path) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("RECURSA_DEBUG_DIR", debug_directory)
        .env("CARGO_TARGET_DIR", target_dir(package));
    command
        .output()
        .expect("run generated grammar fixture with debug emission")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn find_selected_generation(directory: &Path) -> PathBuf {
    let mut found = Vec::new();
    find_named(directory, "recursa-selected.rs", &mut found);
    assert_eq!(found.len(), 1, "one selected Generation: {found:?}");
    found.pop().unwrap()
}

fn find_named(directory: &Path, name: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            find_named(&path, name, found);
        } else if path.file_name().is_some_and(|file| file == name) {
            found.push(path);
        }
    }
}

fn find_one_debug_index(directory: &Path) -> PathBuf {
    let mut found = Vec::new();
    find_named(directory, "index", &mut found);
    assert_eq!(found.len(), 1, "one generated-code debug index: {found:?}");
    found.pop().unwrap()
}

fn force_consumer_recompile(package: &TempDir, marker: &str) {
    let main = package.path().join("src/main.rs");
    let mut source = fs::read_to_string(&main).expect("read downstream consumer");
    source.push_str(&format!("\n// {marker}\n"));
    fs::write(main, source).expect("touch downstream consumer");
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
