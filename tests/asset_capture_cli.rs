//! Asset capture end-to-end tests: use a fake SingleFile, never the network.
#![cfg(unix)]

use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_digital-notes");
const URL: &str = "https://example.org/paper?one=1&two=2";

struct Harness {
    root: TempDir,
}

impl Harness {
    fn new() -> Self {
        let h = Self {
            root: tempfile::tempdir().unwrap(),
        };
        h.ok(&["init"]);
        h
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .arg("--root")
            .arg(self.root())
            .args(args)
            .env_remove("DIGITAL_NOTES_ROOT")
            .env_remove("DIGITAL_NOTES_SINGLE_FILE")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn err(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(!output.status.success(), "expected failure: {args:?}");
        String::from_utf8(output.stderr).unwrap()
    }

    fn work(&self, title: &str) -> String {
        self.ok(&["work", "new", "--title", title, "--id-only"])
            .trim()
            .to_owned()
    }

    fn fake(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root().join(name);
        // A mock can be selected per-command using --single-file.
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    fn capture(&self, work: &str, url: &str, executable: &Path, extra: &[&str]) -> String {
        let mut args = vec![
            "asset",
            "capture",
            url,
            "--work",
            work,
            "--type",
            "academic_paper",
            "--single-file",
        ];
        args.push(executable.to_str().unwrap());
        args.extend_from_slice(extra);
        self.ok(&args).trim().to_owned()
    }

    fn json(&self, id: &str) -> Value {
        serde_json::from_str(&self.ok(&["asset", "show", id, "--json"])).unwrap()
    }

    fn assets(&self) -> Vec<Value> {
        serde_json::from_str(&self.ok(&["asset", "list", "--json"])).unwrap()
    }
}

fn html_asset_count(dir: &Path) -> usize {
    fs::read_dir(dir.join("assets"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == std::ffi::OsStr::new("html"))
        })
        .count()
}

#[test]
fn captures_html_and_registers_valid_sidecar() {
    let h = Harness::new();
    let work = h.work("Read paper");
    let mock = h.fake(
        "single-file.sh",
        r#"[ "$#" -eq 2 ]
[ "$1" = 'https://example.org/paper?one=1&two=2' ]
printf '<!doctype html><html><head><title>Paper</title></head><body>論文</body></html>' > "$2""#,
    );
    let id = h.capture(
        &work,
        URL,
        &mock,
        &["--title", "Japanese paper", "--id-only"],
    );
    assert_eq!(id.len(), 36);
    let asset = h.json(&id);
    assert_eq!(asset["title"], "Japanese paper");
    assert_eq!(asset["asset_type"], "academic_paper");
    assert_eq!(asset["capture_method"], "singlefile");
    assert_eq!(asset["created_by_work_id"], work);
    assert_eq!(asset["related_work_ids"], serde_json::json!([]));
    assert_eq!(asset["url"], URL);
    assert_eq!(asset["created"], asset["updated"]);
    assert_eq!(asset["hash_algorithm"], "sha256");
    assert_eq!(asset["content_hash"].as_str().unwrap().len(), 64);
    let html = fs::read(h.root().join("assets").join(format!("{id}.html"))).unwrap();
    assert!(String::from_utf8(html).unwrap().contains("論文"));
    assert!(h.ok(&["asset", "list"]).contains("Japanese paper"));
    assert_eq!(h.assets().len(), 1);
    assert_eq!(html_asset_count(h.root()), 1);
}

#[test]
fn repeated_capture_is_noop_and_cross_work_capture_links() {
    let h = Harness::new();
    let first_work = h.work("Collect");
    let second_work = h.work("Compare");
    let mock = h.fake("same.sh", "printf '<html>stable content</html>' > \"$2\"");
    let first = h.capture(
        &first_work,
        URL,
        &mock,
        &["--title", "Original", "--id-only"],
    );
    let snapshot = fs::read(h.root().join("assets").join(format!("{first}.json"))).unwrap();
    assert_eq!(
        first,
        h.capture(
            &first_work,
            URL,
            &mock,
            &["--title", "New title ignored", "--id-only"]
        )
    );
    assert_eq!(
        snapshot,
        fs::read(h.root().join("assets").join(format!("{first}.json"))).unwrap()
    );
    assert_eq!(first, h.capture(&second_work, URL, &mock, &["--id-only"]));
    let asset = h.json(&first);
    assert_eq!(asset["created_by_work_id"], first_work);
    assert_eq!(asset["related_work_ids"], serde_json::json!([second_work]));
    assert_eq!(asset["title"], "Original");
    assert_eq!(html_asset_count(h.root()), 1);
}

#[test]
fn changed_html_or_different_url_creates_new_asset() {
    let h = Harness::new();
    let work = h.work("Collection");
    let html1 = h.fake("v1.sh", "printf '<html>version one</html>' > \"$2\"");
    let html2 = h.fake("v2.sh", "printf '<html>version two</html>' > \"$2\"");
    let first = h.capture(&work, URL, &html1, &["--id-only"]);
    let second = h.capture(&work, URL, &html2, &["--id-only"]);
    let third = h.capture(&work, "https://example.org/other", &html1, &["--id-only"]);
    assert_ne!(first, second);
    assert_ne!(first, third);
    assert_eq!(h.assets().len(), 3);
}

#[test]
fn forced_duplicate_requires_explicit_reuse_when_ambiguous() {
    let h = Harness::new();
    let work = h.work("Collect");
    let new_work = h.work("Reuse");
    let mock = h.fake("dup.sh", "printf '<html>unchanged</html>' > \"$2\"");
    let first = h.capture(&work, URL, &mock, &["--id-only"]);
    let second = h.capture(&work, URL, &mock, &["--force-new", "--id-only"]);
    assert_ne!(first, second);
    let args = [
        "asset",
        "capture",
        URL,
        "--work",
        &new_work,
        "--single-file",
        mock.to_str().unwrap(),
        "--id-only",
    ];
    assert!(h.err(&args).contains("multiple matching Assets"));
    assert_eq!(
        first,
        h.capture(&new_work, URL, &mock, &["--reuse", &first, "--id-only"])
    );
    assert_eq!(
        h.json(&first)["related_work_ids"],
        serde_json::json!([new_work])
    );
    assert_eq!(h.json(&second)["related_work_ids"], serde_json::json!([]));
}

#[test]
fn wrong_reuse_id_and_conflicting_flags_fail_without_changing_assets() {
    let h = Harness::new();
    let work = h.work("Work");
    let mock = h.fake("valid.sh", "printf '<html>ok</html>' > \"$2\"");
    let first = h.capture(&work, URL, &mock, &["--id-only"]);
    let other = h.capture(&work, "https://example.org/else", &mock, &["--id-only"]);
    let args = [
        "asset",
        "capture",
        URL,
        "--work",
        &work,
        "--single-file",
        mock.to_str().unwrap(),
        "--reuse",
        &other,
    ];
    assert!(h.err(&args).contains("not a matching Asset"));
    let conflicting = [
        "asset",
        "capture",
        URL,
        "--work",
        &work,
        "--single-file",
        mock.to_str().unwrap(),
        "--force-new",
        "--reuse",
        &first,
    ];
    assert!(!h.run(&conflicting).status.success());
    assert_eq!(h.assets().len(), 2);
}

#[test]
fn missing_work_fails_before_starting_singlefile() {
    let h = Harness::new();
    let marker = h.root().join("invoked");
    let mock = h.fake("side-effect.sh", &format!("touch '{}'", marker.display()));
    let args = [
        "asset",
        "capture",
        URL,
        "--work",
        "missing12",
        "--single-file",
        mock.to_str().unwrap(),
    ];
    assert!(h.err(&args).contains("file not found"));
    assert!(!marker.exists());
    assert_eq!(html_asset_count(h.root()), 0);
}

#[test]
fn failed_singlefile_does_not_publish_any_files() {
    let h = Harness::new();
    let work = h.work("Work");
    let mock = h.fake("fail.sh", "echo 'failed to capture' >&2\nexit 7");
    let args = [
        "asset",
        "capture",
        URL,
        "--work",
        &work,
        "--single-file",
        mock.to_str().unwrap(),
    ];
    assert!(h.err(&args).contains("SingleFile failed"));
    assert_eq!(fs::read_dir(h.root().join("assets")).unwrap().count(), 0);
}

#[test]
fn default_title_is_url_and_ids_can_be_listed() {
    let h = Harness::new();
    let work = h.work("Work");
    let mock = h.fake("valid.sh", "printf '<html>ok</html>' > \"$2\"");
    let id = h.capture(&work, URL, &mock, &["--id-only"]);
    assert_eq!(h.json(&id)["title"], URL);
    assert!(h.ok(&["asset", "list", "--ids"]).contains(&id));
    assert!(h.ok(&["asset", "show", &id]).contains("SHA-256:"));
}

#[test]
fn concurrent_identical_captures_reuse_single_asset() {
    let h = Harness::new();
    let work1 = h.work("First");
    let work2 = h.work("Second");
    let mock = h.fake("concurrent.sh", "printf '<html>race-safe</html>' > \"$2\"");
    let root1 = h.root().to_path_buf();
    let root2 = root1.clone();
    let executable1 = mock.clone();
    let executable2 = mock.clone();
    let thread1 = std::thread::spawn(move || {
        Command::new(BIN)
            .arg("--root")
            .arg(root1)
            .args([
                "asset",
                "capture",
                URL,
                "--work",
                &work1,
                "--single-file",
                executable1.to_str().unwrap(),
                "--id-only",
            ])
            .output()
            .unwrap()
    });
    let thread2 = std::thread::spawn(move || {
        Command::new(BIN)
            .arg("--root")
            .arg(root2)
            .args([
                "asset",
                "capture",
                URL,
                "--work",
                &work2,
                "--single-file",
                executable2.to_str().unwrap(),
                "--id-only",
            ])
            .output()
            .unwrap()
    });
    let first = thread1.join().unwrap();
    let second = thread2.join().unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(h.assets().len(), 1);
}
