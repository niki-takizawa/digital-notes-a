//! End-to-end Work CLI tests, independent of the user's real data.
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_digital-notes");

struct Harness {
    temp: TempDir,
}

impl Harness {
    fn new() -> Self {
        let this = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        this.success(&["init"]);
        this
    }

    fn root(&self) -> &Path {
        self.temp.path()
    }

    fn invoke(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .arg("--root")
            .arg(self.root())
            .args(args)
            .env_remove("DIGITAL_NOTES_ROOT")
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) -> String {
        let output = self.invoke(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn failure(&self, args: &[&str]) -> String {
        let output = self.invoke(args);
        assert!(!output.status.success(), "unexpected success: {args:?}");
        String::from_utf8(output.stderr).unwrap()
    }

    fn new_work(&self, title: &str) -> String {
        self.success(&["work", "new", "--title", title, "--id-only"])
            .trim()
            .to_string()
    }

    fn show(&self, id: &str) -> Value {
        serde_json::from_str(&self.success(&["work", "show", id, "--json"])).unwrap()
    }
}

#[test]
fn create_read_and_list_work_with_schema_fields() {
    let h = Harness::new();
    let body_path = h.root().join("draft.md");
    fs::write(
        &body_path,
        "# 日本語\n\nEvidence will be collected later.\n",
    )
    .unwrap();
    let id = h.success(&[
        "work",
        "new",
        "--title",
        "Read papers",
        "--type",
        "source_collection",
        "--body-file",
        body_path.to_str().unwrap(),
        "--id-only",
    ]);
    let id = id.trim();
    assert_eq!(id.len(), 36, "UUID v4 should be 36 characters");
    let work = h.show(id);
    assert_eq!(work["id"], id);
    assert_eq!(work["schema_version"], 1);
    assert_eq!(work["entity_type"], "work");
    assert_eq!(work["title"], "Read papers");
    assert_eq!(work["status"], "active");
    assert_eq!(work["work_type"], "source_collection");
    assert_eq!(work["created"], work["updated"]);
    assert_eq!(work["source_work_ids"], serde_json::json!([]));
    assert!(work["body"].as_str().unwrap().starts_with("# 日本語"));
    assert!(h.success(&["work", "list"]).contains("Read papers"));
    assert!(h.success(&["work", "show", id]).contains("Read papers"));
    let listed: Value = serde_json::from_str(&h.success(&["work", "list", "--json"])).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[test]
fn list_is_title_first_and_disambiguates_duplicates() {
    let h = Harness::new();
    let second = h.new_work("Zebra");
    let first = h.new_work("Alpha");
    let third = h.new_work("Alpha");
    let output = h.success(&["work", "list"]);
    assert!(output.starts_with("Alpha"), "{output}");
    assert!(output.contains(&format!("Alpha [{first}]")));
    assert!(output.contains(&format!("Alpha [{third}]")));
    assert!(!output.contains(&format!("Zebra [{second}]")), "{output}");
    assert!(
        h.success(&["work", "list", "--ids"])
            .contains(&format!("Zebra [{second}]"))
    );
}

#[test]
fn update_title_type_and_status_reversibly() {
    let h = Harness::new();
    let id = h.new_work("Draft");
    h.success(&[
        "work", "set", &id, "--title", "Revised", "--type", "analysis",
    ]);
    assert_eq!(h.show(&id)["title"], "Revised");
    assert_eq!(h.show(&id)["work_type"], "analysis");
    h.success(&["work", "done", &id]);
    assert_eq!(h.show(&id)["status"], "done");
    let bytes = fs::read(h.root().join("works").join(format!("{id}.json"))).unwrap();
    h.success(&["work", "done", &id]);
    assert_eq!(
        bytes,
        fs::read(h.root().join("works").join(format!("{id}.json"))).unwrap()
    );
    h.success(&["work", "reopen", &id]);
    assert_eq!(h.show(&id)["status"], "active");
}

#[test]
fn source_work_relationships_reject_invalid_references() {
    let h = Harness::new();
    let source = h.new_work("Source task");
    let id = h.new_work("Followup task");
    h.success(&["work", "source", "add", &id, &source]);
    h.success(&["work", "source", "add", &id, &source]);
    assert_eq!(h.show(&id)["source_work_ids"], serde_json::json!([source]));
    assert!(
        h.failure(&["work", "source", "add", &id, &id])
            .contains("cannot reference itself")
    );
    assert!(
        h.failure(&["work", "source", "add", &id, "missing00"])
            .contains("error:")
    );
    h.success(&["work", "source", "remove", &id, &source]);
    assert_eq!(h.show(&id)["source_work_ids"], serde_json::json!([]));
    assert!(
        h.failure(&["work", "source", "remove", &id, &source])
            .contains("not referenced")
    );
    let from_cli = h.success(&[
        "work",
        "new",
        "--title",
        "Derived",
        "--source",
        &id,
        "--id-only",
    ]);
    assert_eq!(
        h.show(from_cli.trim())["source_work_ids"],
        serde_json::json!([id])
    );
}

#[test]
fn editing_body_from_file_does_not_change_immutable_fields() {
    let h = Harness::new();
    let id = h.new_work("Draft");
    let old = h.show(&id);
    let file = h.root().join("work-body.md");
    fs::write(&file, "New **Markdown** 🌱\n").unwrap();
    h.success(&["work", "edit", &id, "--body-file", file.to_str().unwrap()]);
    let new = h.show(&id);
    assert_eq!(new["body"], "New **Markdown** 🌱\n");
    assert_eq!(new["id"], old["id"]);
    assert_eq!(new["created"], old["created"]);
    assert!(new["updated"].as_str().unwrap().len() >= 20);
}

#[test]
fn bad_input_and_corrupt_repositories_fail_without_silent_success() {
    let h = Harness::new();
    assert!(
        h.failure(&["work", "new", "--title", "  "])
            .contains("error:")
    );
    assert!(
        h.failure(&["work", "new", "--title", "Good", "--type", "NotSnake"])
            .contains("error:")
    );
    assert!(
        h.failure(&["work", "new", "--title", "Invalid", "--source", "unknown00"])
            .contains("error:")
    );
    assert_eq!(h.success(&["work", "list", "--json"]).trim(), "[]");
    let id = h.new_work("Valid");
    assert!(h.failure(&["work", "set", &id]).contains("specify --title"));
    fs::write(
        h.root().join("works").join(format!("{id}.json")),
        b"{broken",
    )
    .unwrap();
    assert!(h.failure(&["work", "list"]).contains("error:"));
}

#[cfg(unix)]
#[test]
fn external_editor_round_trip_and_failure_preserves_original() {
    use std::os::unix::fs::PermissionsExt;
    let h = Harness::new();
    let id = h.new_work("Editing test");
    let editor = h.root().join("fake-editor.sh");
    fs::write(
        &editor,
        "#!/bin/sh\nprintf 'Added in editor\\n' >> \"$1\"\n",
    )
    .unwrap();
    let mut mode = fs::metadata(&editor).unwrap().permissions();
    mode.set_mode(0o700);
    fs::set_permissions(&editor, mode).unwrap();
    let output = Command::new(BIN)
        .arg("--root")
        .arg(h.root())
        .args(["work", "edit", &id])
        .env("VISUAL", editor.as_os_str())
        .env_remove("EDITOR")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(h.show(&id)["body"], "Added in editor\n");
    fs::write(&editor, "#!/bin/sh\nprintf 'Unsaved' >> \"$1\"\nexit 9\n").unwrap();
    let failed = Command::new(BIN)
        .arg("--root")
        .arg(h.root())
        .args(["work", "edit", &id])
        .env("VISUAL", editor.as_os_str())
        .env_remove("EDITOR")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let error = String::from_utf8_lossy(&failed.stderr);
    assert!(error.contains("preserved at"), "{error}");
    assert_eq!(h.show(&id)["body"], "Added in editor\n");
    // Cleanup the intentionally preserved draft.
    if let Some(path) = error.trim().split("preserved at ").nth(1) {
        fs::remove_file(path).unwrap();
    }
}
