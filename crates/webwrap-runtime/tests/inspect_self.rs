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
