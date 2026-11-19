//! Native integration tests. These need the `webwrap-pe-specialize` binary
//! from `spikes/windows-pe-specialization`, which is still in the workspace
//! precisely because spike removal is staged per porter. Sub-project C
//! replaces `specializer_path` with the real generator and nothing else here
//! changes.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// A specification whose appId matches its launchUrl, as the invariant
/// requires. The identity of `https://www.photopea.com/` is
/// `873756fbe97a1384`, pinned by `webwrap-spec`'s contract vectors.
const PHOTOPEA_SPEC: &str = r#"{
  "schemaVersion": 1,
  "appId": "873756fbe97a1384",
  "displayName": "Photopea",
  "slug": "photopea",
  "launchUrl": "https://www.photopea.com/",
  "trustedOrigins": ["https://www.photopea.com"],
  "adapter": "photopea",
  "fileAssociations": [
    {
      "extension": ".psd",
      "mimeType": "image/vnd.adobe.photoshop",
      "description": "Photoshop document"
    }
  ],
  "dnsMode": "system",
  "appVersion": "1.0.0"
}"#;

/// The identity of `https://twitter.com/` is `2fa5e3f40150278f`.
const TWITTER_SPEC: &str = r#"{
  "schemaVersion": 1,
  "appId": "2fa5e3f40150278f",
  "displayName": "Twitter",
  "slug": "twitter",
  "launchUrl": "https://twitter.com/",
  "trustedOrigins": ["https://twitter.com"],
  "adapter": "none",
  "fileAssociations": [],
  "dnsMode": "system",
  "appVersion": "1.0.0"
}"#;

fn unique(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "webwrap-runtime-{label}-{}-{nonce}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Directory holding the built `webwrap-runtime.exe`, which is also where
/// cargo puts `webwrap-pe-specialize.exe`. Derived from the binary cargo
/// names, not from this test's own path: since cargo 1.100's build-dir layout
/// test binaries live under `build/<pkg>/<hash>/out/`, not beside the bins.
fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_webwrap-runtime"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn template_path() -> PathBuf {
    let path = build_dir().join("webwrap-runtime.exe");
    assert!(
        path.is_file(),
        "template not built at {}; run cargo build -p webwrap-runtime --target <triple>",
        path.display()
    );
    path
}

fn specializer_path() -> PathBuf {
    let path = build_dir().join("webwrap-pe-specialize.exe");
    assert!(
        path.is_file(),
        "specializer not built at {}; run cargo build -p windows-pe-specialization-spike --target <triple>",
        path.display()
    );
    path
}

/// A minimal valid PNG for the icon argument: 1x1 opaque black.
const ICON_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Specialize the built template with the given specification and return the
/// path of the produced executable.
pub fn specialize_fixture(spec_json: &str, display_name: &str, version: &str) -> PathBuf {
    let dir = unique("case");
    fs::create_dir_all(&dir).unwrap();
    let spec = dir.join("spec.json");
    fs::write(&spec, spec_json).unwrap();
    let icon = dir.join("icon.png");
    fs::write(&icon, ICON_PNG).unwrap();
    let output = dir.join("app.exe");

    let status = Command::new(specializer_path())
        .args([
            "specialize",
            "--input",
            template_path().to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--spec",
            spec.to_str().unwrap(),
            "--icon",
            icon.to_str().unwrap(),
            "--display-name",
            display_name,
            "--version",
            version,
        ])
        .status()
        .unwrap();
    assert!(status.success(), "specialize failed with {status:?}");
    assert!(output.is_file());
    output
}

fn inspect_with_specializer(exe: &Path) -> serde_json::Value {
    let out = Command::new(specializer_path())
        .args(["inspect", "--input", exe.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "inspect failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_template_carries_the_resources_specialization_needs() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = inspect_with_specializer(&exe);

    assert_eq!(
        report["specBytes"].as_u64().unwrap() as usize,
        PHOTOPEA_SPEC.len()
    );
    assert_eq!(report["iconPresent"], serde_json::json!(true));
    // Proves the version strings were actually replaced rather than leaving a
    // stale value from the template's own VERSIONINFO.
    assert_eq!(report["fileDescription"], serde_json::json!("Photopea"));
    assert_eq!(report["productVersion"], serde_json::json!("1.0.0"));
}

fn inspect_self(exe: &Path, check_urls: &[&str]) -> serde_json::Value {
    let mut command = Command::new(exe);
    command.arg("--inspect-self");
    for url in check_urls {
        command.args(["--check-url", url]);
    }
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "--inspect-self failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn a_specialized_executable_reports_its_own_specification() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = inspect_self(&exe, &[]);

    assert_eq!(report["schemaVersion"], serde_json::json!(1));
    assert_eq!(report["appId"], serde_json::json!("873756fbe97a1384"));
    assert_eq!(report["displayName"], serde_json::json!("Photopea"));
    assert_eq!(report["slug"], serde_json::json!("photopea"));
    assert_eq!(
        report["launchUrl"],
        serde_json::json!("https://www.photopea.com/")
    );
    assert_eq!(
        report["trustedOrigins"],
        serde_json::json!(["https://www.photopea.com"])
    );
    assert_eq!(report["adapter"], serde_json::json!("photopea"));
    assert_eq!(report["dnsMode"], serde_json::json!("system"));
    assert_eq!(report["appVersion"], serde_json::json!("1.0.0"));
    assert_eq!(
        report["fileAssociations"][0]["extension"],
        serde_json::json!(".psd")
    );
    // Derived from the running binary, never stored in the specification, so
    // it cannot disagree with the actual executable.
    let arch = report["runtimeArch"].as_str().unwrap();
    assert!(
        arch == "x86_64" || arch == "aarch64",
        "unexpected arch {arch}"
    );
}

#[test]
fn renaming_and_moving_the_executable_does_not_break_the_read() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let moved = unique("moved");
    fs::create_dir_all(&moved).unwrap();
    let renamed = moved.join("totally-different-name.exe");
    fs::rename(&exe, &renamed).unwrap();

    // The resource is read from the loaded image, not by reopening the file
    // by its original path.
    let report = inspect_self(&renamed, &[]);
    assert_eq!(report["appId"], serde_json::json!("873756fbe97a1384"));
}

#[test]
fn an_unspecialized_template_refuses_to_run() {
    let out = Command::new(template_path())
        .arg("--inspect-self")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("never specialized"),
        "unhelpful message: {stderr}"
    );
}

#[test]
fn unknown_arguments_are_a_usage_error() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    for args in [
        vec!["--nonsense"],
        vec!["some-file.psd"],
        vec!["--inspect-self", "--check-url"],
        vec!["--inspect-self", "--check-url", "--nonsense"],
        vec!["--inspect-self", "extra"],
    ] {
        let out = Command::new(&exe).args(&args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "args {args:?} should be usage errors"
        );
    }
}

#[test]
fn a_specification_that_fails_validation_is_rejected_at_startup() {
    // appId does not match launchUrl, so invariant 2 must reject it. The
    // specializer's own weaker validator accepts this, which is exactly why
    // the runtime revalidates.
    let mismatched = PHOTOPEA_SPEC.replace("873756fbe97a1384", "0000000000000000");
    let exe = specialize_fixture(&mismatched, "Photopea", "1.0.0");
    let out = Command::new(&exe).arg("--inspect-self").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("appId"), "unhelpful message: {stderr}");
}

#[test]
fn gate_11_two_apps_use_separate_webview2_data_directories() {
    let photopea = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let twitter = specialize_fixture(TWITTER_SPEC, "Twitter", "1.0.0");

    let a = inspect_self(&photopea, &[]);
    let b = inspect_self(&twitter, &[]);

    let dir_a = a["webview2DataDir"].as_str().unwrap();
    let dir_b = b["webview2DataDir"].as_str().unwrap();

    assert_ne!(dir_a, dir_b, "generated apps must not share storage");
    assert!(dir_a.ends_with("873756fbe97a1384"), "{dir_a}");
    assert!(dir_b.ends_with("2fa5e3f40150278f"), "{dir_b}");
    assert!(dir_a.contains("WebWrap"), "{dir_a}");

    // The directory is derived from the identity, and the identity is pinned
    // to the launch URL by the specification's own invariant, so storage
    // cannot drift away from the site it belongs to.
    assert_eq!(a["appId"], serde_json::json!("873756fbe97a1384"));
    assert_eq!(b["appId"], serde_json::json!("2fa5e3f40150278f"));
}

#[test]
fn the_data_directory_sits_under_localappdata() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = inspect_self(&exe, &[]);
    let local = std::env::var("LOCALAPPDATA").unwrap();
    let dir = report["webview2DataDir"].as_str().unwrap();
    assert!(dir.starts_with(&local), "{dir} is not under {local}");
}
