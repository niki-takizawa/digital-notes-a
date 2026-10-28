//! SingleFile adapter tests use a fake executable, never network or a browser.
#![cfg(unix)]

use digital_notes::{
    capture::{CaptureError, SingleFileCapture},
    model::HttpUrl,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use tempfile::TempDir;

// Write mock executables in a child process to avoid intermittent ETXTBSY
// when parallel Rust tests spawn other processes on Linux.
fn write_script(path: &Path, contents: &str) {
    let mut writer = Command::new("/bin/sh")
        .arg("-c")
        .arg("cat > \"$1\" && chmod 700 \"$1\"")
        .arg("sh")
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    writer
        .stdin
        .take()
        .unwrap()
        .write_all(contents.as_bytes())
        .unwrap();
    assert!(writer.wait().unwrap().success());
}

fn executable(dir: &Path, script: &str) -> PathBuf {
    let path = dir.join("fake-single-file.sh");
    write_script(&path, &format!("#!/bin/sh\nset -eu\n{script}\n"));
    path
}

fn url(input: &str) -> HttpUrl {
    input.parse().unwrap()
}

#[test]
fn returns_singlefile_html_without_writing_to_repository() {
    let temp = TempDir::new().unwrap();
    let mock = executable(
        temp.path(),
        r#"[ "$#" -eq 2 ]
[ "$1" = 'https://example.org/a?x=1&y=2' ]
printf '<!DOCTYPE html><html><body>研究</body></html>' > "$2""#,
    );
    let result = SingleFileCapture::new(mock)
        .capture(&url("https://example.org/a?x=1&y=2"))
        .unwrap();
    assert_eq!(
        std::str::from_utf8(&result).unwrap(),
        "<!DOCTYPE html><html><body>研究</body></html>"
    );
    assert_eq!(
        fs::read_dir(temp.path()).unwrap().count(),
        1,
        "no asset or output created next to mock"
    );
}

#[test]
fn nonzero_exit_reports_failure_and_stderr() {
    let temp = TempDir::new().unwrap();
    let mock = executable(temp.path(), "echo 'browser missing' >&2\nexit 7");
    let error = SingleFileCapture::new(mock)
        .capture(&url("https://example.org/"))
        .unwrap_err();
    match error {
        CaptureError::Failed { status, stderr } => {
            assert_eq!(status.code(), Some(7));
            assert!(stderr.contains("browser missing"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn missing_output_is_rejected_even_on_successful_exit() {
    let temp = TempDir::new().unwrap();
    let mock = executable(temp.path(), ":");
    assert!(matches!(
        SingleFileCapture::new(mock).capture(&url("https://example.org/")),
        Err(CaptureError::MissingOutput)
    ));
}

#[test]
fn empty_output_is_rejected() {
    let temp = TempDir::new().unwrap();
    let mock = executable(temp.path(), r#": > "$2""#);
    assert!(matches!(
        SingleFileCapture::new(mock).capture(&url("https://example.org/")),
        Err(CaptureError::EmptyOutput)
    ));
}

#[test]
fn text_and_non_utf8_are_rejected() {
    let temp = TempDir::new().unwrap();
    let mock = executable(temp.path(), r#"printf 'not html' > "$2""#);

    let result = SingleFileCapture::new(mock.clone()).capture(&url("http://example.org/"));
    assert!(
        matches!(&result, Err(CaptureError::NotHtml)),
        "expected NotHtml, got: {result:?}"
    );

    write_script(
        &mock,
        r#"#!/bin/sh
printf '\377' > "$2"
"#,
    );

    let result = SingleFileCapture::new(mock).capture(&url("http://example.org/"));
    assert!(
        matches!(&result, Err(CaptureError::InvalidUtf8)),
        "expected InvalidUtf8, got: {result:?}"
    );
}

#[test]
fn oversize_output_is_rejected() {
    let temp = TempDir::new().unwrap();
    let mock = executable(temp.path(), r#"printf '<html>oversize</html>' > "$2""#);
    let result = SingleFileCapture::new(mock)
        .with_max_bytes(10)
        .capture(&url("https://example.org/"));
    assert!(
        matches!(result, Err(CaptureError::TooLarge { limit: 10 })),
        "expected TooLarge with limit 10, got: {result:?}"
    );
}

#[test]
fn refuses_symlink_output() {
    let temp = TempDir::new().unwrap();
    let mock = executable(temp.path(), r#"ln -s /dev/null "$2""#);
    assert!(matches!(
        SingleFileCapture::new(mock).capture(&url("https://example.org/")),
        Err(CaptureError::NotRegularFile)
    ));
}

#[test]
fn times_out_and_reaps_direct_process() {
    let temp = TempDir::new().unwrap();
    let mock = executable(temp.path(), "while :; do :; done");
    let result = SingleFileCapture::new(mock)
        .with_timeout(Duration::from_millis(75))
        .capture(&url("https://example.org/"));
    assert!(
        matches!(result, Err(CaptureError::TimedOut(_))),
        "{result:?}"
    );
}

#[test]
fn missing_executable_reports_launch_error() {
    let temp = TempDir::new().unwrap();
    let mock = temp.path().join("does-not-exist");
    assert!(matches!(
        SingleFileCapture::new(mock).capture(&url("https://example.org/")),
        Err(CaptureError::Io {
            operation: "launch",
            ..
        })
    ));
}
