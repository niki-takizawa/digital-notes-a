//! TUI must not put the terminal in raw mode in noninteractive environments.

use std::process::Command;

#[test]
fn tui_subcommand_is_documented() {
    let output = Command::new(env!("CARGO_BIN_EXE_digital-notes"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("tui"));
}

#[test]
fn tui_requires_an_interactive_terminal() {
    let root = tempfile::tempdir().unwrap();
    digital_notes::repository::Repository::init(root.path()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_digital-notes"))
        .arg("--root")
        .arg(root.path())
        .arg("tui")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("interactive terminal"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
