//! The generator binary's contract: exit codes and JSON shapes. Runs with
//! WEBWRAP_TEMPLATE_X64/ARM64 set, so the binary under test has them embedded.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

fn temp_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "webwrap-generate-cli-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn icon(dir: &Path) -> PathBuf {
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(64, 64, image::Rgba([229, 57, 53, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let path = dir.join("icon.png");
    std::fs::write(&path, png.into_inner()).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_webwrap-generate"))
        .args(args)
        .output()
        .unwrap()
}

fn json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn usage_errors_exit_2() {
    for args in [
        vec![],
        vec!["frobnicate"],
        vec!["inspect"],
        vec!["inspect", "--input"],
        vec!["inspect", "--input", "--url"],
        vec!["inspect", "--input", "a.exe", "extra"],
        vec!["inspect", "--input", "a.exe", "--url", "x"],
        vec!["generate", "--url", "twitter.com"],
        vec!["generate", "--assoc", ".md", "text/markdown"],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn invalid_input_exits_2_and_unreadable_files_exit_1() {
    let dir = temp_dir();
    let icon = icon(&dir);
    let out = dir.to_str().unwrap();
    let base = ["generate", "--name", "Twitter", "--out", out];

    let mut http = base.to_vec();
    http.extend([
        "--url",
        "http://twitter.com/",
        "--icon",
        icon.to_str().unwrap(),
    ]);
    assert_eq!(run(&http).status.code(), Some(2));

    let mut lone_template = http.clone();
    lone_template[6] = "twitter.com";
    lone_template.extend(["--template-x64", "a.exe"]);
    assert_eq!(run(&lone_template).status.code(), Some(2));

    let mut missing_icon = base.to_vec();
    missing_icon.extend(["--url", "twitter.com", "--icon", "no-such-icon.png"]);
    assert_eq!(run(&missing_icon).status.code(), Some(1));

    assert_eq!(
        run(&["inspect", "--input", "no-such.exe"]).status.code(),
        Some(1)
    );
}

#[test]
fn generate_and_inspect_report_json_using_the_embedded_templates() {
    let dir = temp_dir();
    let icon = icon(&dir);
    let report = json(&run(&[
        "generate",
        "--url",
        "www.photopea.com",
        "--name",
        "Photopea",
        "--icon",
        icon.to_str().unwrap(),
        "--out",
        dir.to_str().unwrap(),
        "--adapter",
        "photopea",
        "--assoc",
        ".psd",
        "image/vnd.adobe.photoshop",
        "Photoshop document",
    ]));
    assert_eq!(report["appId"], "873756fbe97a1384");
    assert_eq!(report["slug"], "photopea");
    let outputs = report["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 2);
    for (output, arch) in outputs.iter().zip(["x64", "arm64"]) {
        assert_eq!(output["arch"], arch);
        assert_eq!(output["replaced"], "none");
        let path = output["path"].as_str().unwrap();
        assert!(path.ends_with(&format!("photopea-{arch}.exe")), "{path}");

        let inspected = json(&run(&["inspect", "--input", path]));
        assert_eq!(inspected["arch"], arch);
        assert_eq!(inspected["appId"], "873756fbe97a1384");
        assert_eq!(inspected["iconPresent"], true);
        assert_eq!(inspected["fileDescription"], "Photopea");
        assert_eq!(inspected["productVersion"], "1.0.0");
    }
}
