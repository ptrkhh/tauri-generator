use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn temp_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "webwrap-pe-cli-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_webwrap-pe-specialize"))
}

#[test]
fn unknown_flag_exits_2() {
    let status = bin().args(["specialize", "--bogus", "x"]).status().unwrap();
    assert_eq!(status.code(), Some(2));
}

#[test]
fn extra_positional_argument_exits_2() {
    let status = bin()
        .args([
            "specialize",
            "--input",
            "in.exe",
            "--output",
            "out.exe",
            "--spec",
            "spec.json",
            "--icon",
            "icon.png",
            "--display-name",
            "A",
            "--version",
            "1",
            "extra",
        ])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

#[test]
fn missing_spec_file_exits_1() {
    let temp = temp_dir();
    let missing_spec = temp.join("missing-spec.json");
    let status = bin()
        .args([
            "specialize",
            "--input",
            "in.exe",
            "--output",
            "out.exe",
            "--spec",
            missing_spec.to_str().unwrap(),
            "--icon",
            "icon.png",
            "--display-name",
            "A",
            "--version",
            "1",
        ])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn malformed_spec_content_exits_2() {
    let temp = temp_dir();
    let spec = temp.join("bad-spec.json");
    std::fs::write(&spec, b"not json").unwrap();
    let status = bin()
        .args([
            "specialize",
            "--input",
            "in.exe",
            "--output",
            "out.exe",
            "--spec",
            spec.to_str().unwrap(),
            "--icon",
            "icon.png",
            "--display-name",
            "A",
            "--version",
            "1",
        ])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
    let _ = std::fs::remove_dir_all(temp);
}
