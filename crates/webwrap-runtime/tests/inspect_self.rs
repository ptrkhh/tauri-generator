//! Native integration tests. Fixtures are specialized with
//! `webwrap_pack::specialize_raw`, the unchecked primitive, so tests can also
//! embed specifications the runtime must reject.

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

fn template_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_webwrap-runtime"))
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
    let template = fs::read(template_path()).unwrap();
    let app = webwrap_pack::specialize_raw(
        &template,
        spec_json.as_bytes(),
        ICON_PNG,
        display_name,
        version,
    )
    .unwrap();
    let output = dir.join("app.exe");
    fs::write(&output, app).unwrap();
    output
}

#[test]
fn the_template_carries_the_resources_specialization_needs() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = webwrap_pack::inspect(&fs::read(&exe).unwrap()).unwrap();

    assert_eq!(report.spec.as_deref(), Some(PHOTOPEA_SPEC.as_bytes()));
    assert!(report.icon_present);
    // Proves the version strings were actually replaced rather than leaving a
    // stale value from the template's own VERSIONINFO.
    assert_eq!(report.file_description.as_deref(), Some("Photopea"));
    assert_eq!(report.product_version.as_deref(), Some("1.0.0"));
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

    // A lone UTF-16 surrogate is a valid Windows argument but not a String.
    use std::os::windows::ffi::OsStringExt;
    let lone_surrogate = std::ffi::OsString::from_wide(&[0xD800]);
    let out = Command::new(&exe).arg(lone_surrogate).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "non-Unicode argument");
}

#[test]
fn a_specification_that_fails_validation_is_rejected_at_startup() {
    // appId does not match launchUrl, so invariant 2 must reject it. The
    // generator never embeds such a spec, but specialize_raw does, and an app
    // could be edited by other tools; the runtime is the authority.
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

#[test]
fn gate_14_navigation_policy_is_enforced_by_the_real_binary() {
    let exe = specialize_fixture(PHOTOPEA_SPEC, "Photopea", "1.0.0");
    let report = inspect_self(
        &exe,
        &[
            "https://www.photopea.com/",
            "https://accounts.google.com/o/oauth2/auth",
            "http://www.photopea.com/",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "not a url",
        ],
    );

    let verdicts = report["navVerdicts"].as_array().unwrap();
    assert_eq!(verdicts.len(), 6);

    let verdict_for = |url: &str| -> String {
        verdicts
            .iter()
            .find(|entry| entry["url"] == serde_json::json!(url))
            .unwrap_or_else(|| panic!("no verdict for {url}"))["verdict"]
            .as_str()
            .unwrap()
            .to_string()
    };

    assert_eq!(verdict_for("https://www.photopea.com/"), "allow");
    // An untrusted https origin is still allowed to navigate, or login breaks.
    assert_eq!(
        verdict_for("https://accounts.google.com/o/oauth2/auth"),
        "allow"
    );
    assert_eq!(verdict_for("http://www.photopea.com/"), "block");
    assert_eq!(verdict_for("file:///C:/Windows/win.ini"), "block");
    assert_eq!(verdict_for("javascript:alert(1)"), "block");
    assert_eq!(verdict_for("not a url"), "block");
}
