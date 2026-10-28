//! End-to-end smoke tests for the binary's arguments and exit statuses.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);
const BIN: &str = env!("CARGO_BIN_EXE_digital-notes");

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let seq = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "digital-notes-cli-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove test directory");
    }
}

fn invoke(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("DIGITAL_NOTES_ROOT")
        .output()
        .expect("run digital-notes")
}

#[test]
fn help_and_version_succeed_without_creating_a_repository() {
    let help = invoke(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("Usage:"), "{text}");
    assert!(text.contains("init"), "{text}");
    assert!(text.contains("--root"), "{text}");

    let version = invoke(&["--version"]);
    assert!(version.status.success());
    assert!(
        String::from_utf8(version.stdout)
            .unwrap()
            .contains(env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn invalid_or_missing_subcommand_reports_usage_error() {
    for (args, expected) in [
        (&[][..], "Usage:"),
        (&["unknown"][..], "unknown"),
        (&["--root"][..], "--root"),
    ] {
        let output = invoke(args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(2),
            "args: {args:?}; stderr: {stderr}"
        );
        assert!(
            stderr.contains(expected),
            "args: {args:?}; expected {expected:?}; stderr: {stderr}"
        );
    }
}

#[test]
fn init_creates_expected_directories_and_is_idempotent() {
    let temp = TestRoot::new();
    let root = temp.path().join("project");
    for _ in 0..2 {
        let output = Command::new(BIN)
            .args(["init", "--root"])
            .arg(&root)
            .env_remove("DIGITAL_NOTES_ROOT")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Initialized"));
        for dir in [".pri", "works", "assets", "cards"] {
            assert!(root.join(dir).is_dir(), "{dir}");
        }
        // Existing contents must survive repeated initialization.
        fs::write(root.join("works/keep.txt"), b"keep").unwrap();
    }
    assert_eq!(fs::read(root.join("works/keep.txt")).unwrap(), b"keep");
}

#[test]
fn root_env_fallback_and_explicit_option_override() {
    let temp = TestRoot::new();
    let from_env = temp.path().join("from-env");
    let from_flag = temp.path().join("from-flag");

    let output = Command::new(BIN)
        .arg("init")
        .env("DIGITAL_NOTES_ROOT", &from_env)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(from_env.join("works").is_dir());

    let output = Command::new(BIN)
        .arg("--root")
        .arg(&from_flag)
        .arg("init")
        .env("DIGITAL_NOTES_ROOT", temp.path().join("not-used"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(from_flag.join("works").is_dir());
    assert!(!temp.path().join("not-used").exists());
}

#[test]
fn init_defaults_to_current_directory() {
    let temp = TestRoot::new();
    let output = Command::new(BIN)
        .arg("init")
        .current_dir(temp.path())
        .env_remove("DIGITAL_NOTES_ROOT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp.path().join("works").is_dir());
}

#[test]
fn initialization_failure_is_nonzero_and_diagnostic() {
    let temp = TestRoot::new();
    let not_a_directory = temp.path().join("file");
    fs::write(&not_a_directory, b"not a directory").unwrap();
    let output = Command::new(BIN)
        .arg("init")
        .arg("--root")
        .arg(&not_a_directory)
        .env_remove("DIGITAL_NOTES_ROOT")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
}
