// INPUT:  std::{fs, process::Command, sync::OnceLock}, alva_sandbox_wasm, tempfile
// OUTPUT: Integration coverage for the public WASIp1 runner seam
// POS:    Builds the fixture on demand and verifies only guest-visible output plus host filesystem effects.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use alva_sandbox_wasm::{run_module, Grant, RunRequest};

static FIXTURE_WASM: OnceLock<Vec<u8>> = OnceLock::new();

#[test]
fn granted_directory_supports_crud_and_blocks_escape() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    let outside = root.path().join("outside");
    fs::create_dir_all(&granted).expect("create granted directory");
    fs::create_dir_all(&outside).expect("create outside directory");
    fs::write(granted.join("existing.txt"), "before").expect("seed granted file");
    let outside_secret = outside.join("secret.txt");
    fs::write(&outside_secret, "must stay hidden").expect("seed outside file");

    let outcome = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted.clone(), "/work")],
        args: vec![
            outside_secret.to_string_lossy().into_owned(),
            "job-arg".into(),
        ],
    })
    .expect("run fixture module");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty(), "stderr: {}", outcome.stderr);
    assert!(outcome.stdout.contains("READ existing.txt: before"));
    assert!(outcome
        .stdout
        .contains("LIST /work: [\"existing.txt\", \"new.txt\"]"));
    assert!(outcome.stdout.contains("ARGS: job-arg"));
    assert!(outcome.stdout.contains("ESCAPE-1 blocked: NotFound"));
    assert!(outcome
        .stdout
        .contains("ESCAPE-2 blocked: PermissionDenied"));
    assert!(!outcome.stdout.contains("!!!"));

    assert_eq!(
        fs::read_to_string(granted.join("existing.txt")).expect("read overwritten file"),
        "before+modified"
    );
    assert_eq!(
        fs::read_to_string(granted.join("subdir/renamed.txt")).expect("read renamed file"),
        "created-in-sandbox"
    );
    assert!(granted.join("subdir").is_dir());
    assert!(!granted.join("new.txt").exists());
    assert!(!granted.join("tmp-delete-me.txt").exists());
    assert_eq!(
        fs::read_to_string(&outside_secret).expect("read outside file after guest run"),
        "must stay hidden"
    );
}

#[test]
fn wasi_exit_and_stderr_are_returned_as_an_outcome() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    let outside_secret = root.path().join("outside-secret.txt");
    fs::create_dir_all(&granted).expect("create granted directory");
    fs::write(granted.join("existing.txt"), "before").expect("seed granted file");
    fs::write(&outside_secret, "must stay hidden").expect("seed outside file");

    let outcome = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted, "/work")],
        args: vec![
            outside_secret.to_string_lossy().into_owned(),
            "exit-7".into(),
        ],
    })
    .expect("WASI proc_exit is a process outcome, not a runner failure");

    assert_eq!(outcome.exit_code, 7);
    assert!(outcome.stderr.contains("fixture requested exit 7"));
}

#[test]
fn read_only_grant_blocks_mutation() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    let outside_secret = root.path().join("outside-secret.txt");
    fs::create_dir_all(&granted).expect("create granted directory");
    fs::write(granted.join("existing.txt"), "before").expect("seed granted file");
    fs::write(&outside_secret, "must stay hidden").expect("seed outside file");

    // The fixture's first action is `fs::write("/work/new.txt", ...)`; under a
    // read-only mount that write must fail, so the guest traps rather than
    // exiting cleanly, and the host directory is left untouched.
    let result = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_only(granted.clone(), "/work")],
        args: vec![outside_secret.to_string_lossy().into_owned(), "job".into()],
    });

    assert!(
        result.is_err() || result.as_ref().is_ok_and(|o| o.exit_code != 0),
        "read-only mount let the guest exit cleanly: {result:?}"
    );
    assert!(
        !granted.join("new.txt").exists(),
        "read-only mount allowed a new file to be created"
    );
    assert_eq!(
        fs::read_to_string(granted.join("existing.txt")).expect("existing file survives"),
        "before",
        "read-only mount allowed an overwrite"
    );
}

fn fixture_wasm() -> &'static [u8] {
    FIXTURE_WASM.get_or_init(build_fixture).as_slice()
}

fn build_fixture() -> Vec<u8> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_manifest = manifest_dir.join("tests/fixtures/fs-guest/Cargo.toml");
    let target_dir = tempfile::tempdir().expect("create fixture target directory");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));

    let output = Command::new(cargo)
        .arg("build")
        .arg("--locked")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip1")
        .arg("--manifest-path")
        .arg(&fixture_manifest)
        .arg("--target-dir")
        .arg(target_dir.path())
        .output()
        .expect("spawn cargo to build WASIp1 fixture");

    assert!(
        output.status.success(),
        "fixture build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let wasm = fixture_artifact(target_dir.path());
    fs::read(&wasm).unwrap_or_else(|error| panic!("read fixture wasm at {wasm:?}: {error}"))
}

fn fixture_artifact(target_dir: &Path) -> PathBuf {
    target_dir
        .join("wasm32-wasip1")
        .join("release")
        .join("alva-sandbox-wasm-fixture.wasm")
}
