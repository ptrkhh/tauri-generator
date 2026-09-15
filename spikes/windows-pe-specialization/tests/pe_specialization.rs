use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use editpe::Image;
use windows_pe_specialization_spike::pe::{edit_resources, read_metadata, specialize};

fn temp_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "webwrap-pe-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

#[test]
fn embeds_spec_icon_and_version_metadata() {
    let spec = br#"{"schemaVersion":1,"appId":"a123","displayName":"Photopea","slug":"photopea","launchUrl":"https://www.photopea.com/"}"#;
    let input = std::env::current_exe().unwrap();
    let icon = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../photopea-windows-writeback/icons/icon.png");
    let mut image = Image::parse_file(&input).unwrap();
    edit_resources(&mut image, spec, &icon, "Photopea", "1.2.3").unwrap();
    let output = temp_dir().join("specialized.exe");
    image.write_file(&output).unwrap();
    let metadata = read_metadata(&output).unwrap();
    assert_eq!(metadata.spec, spec);
    assert!(metadata.icon_present);
    assert_eq!(metadata.file_description, "Photopea");
    assert_eq!(metadata.product_version, "1.2.3");
    let _ = std::fs::remove_dir_all(output.parent().unwrap());
}

#[test]
fn failed_specialization_preserves_existing_output_and_cleans_stage() {
    let temp = temp_dir();
    let bad_input = temp.join("not-a-pe.exe");
    let output = temp.join("twitter.exe");
    let icon = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../photopea-windows-writeback/icons/icon.png");
    std::fs::write(&output, b"previous output").unwrap();
    std::fs::write(&bad_input, b"not a PE").unwrap();
    let before = std::fs::read(&output).unwrap();
    assert!(specialize(&bad_input, &output, SPEC, &icon, "Photopea", "1.2.3").is_err());
    assert_eq!(std::fs::read(&output).unwrap(), before);
    assert!(!std::fs::read_dir(&temp)
        .unwrap()
        .map(Result::unwrap)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".webwrap-pe-")));
    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn specializes_and_replaces_output_atomically() {
    let temp = temp_dir();
    let input = std::env::current_exe().unwrap();
    let output = temp.join("twitter.exe");
    let icon = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../photopea-windows-writeback/icons/icon.png");
    let metadata = specialize(&input, &output, SPEC, &icon, "Twitter", "1.0.0").unwrap();
    assert_eq!(metadata.spec, SPEC);
    assert_eq!(metadata.file_description, "Twitter");
    assert_eq!(read_metadata(&output).unwrap().product_version, "1.0.0");
    let second = specialize(&input, &output, SPEC, &icon, "Changed", "2.0.0").unwrap();
    assert_eq!(second.file_description, "Changed");
    assert_eq!(read_metadata(&output).unwrap().product_version, "2.0.0");
    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn invalid_icon_and_same_input_output_are_rejected_without_output() {
    let temp = temp_dir();
    let input = std::env::current_exe().unwrap();
    let output = temp.join("twitter.exe");
    let missing_icon = temp.join("missing.png");
    assert!(specialize(&input, &output, SPEC, &missing_icon, "Twitter", "1.0.0").is_err());
    assert!(!output.exists());
    let icon = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../photopea-windows-writeback/icons/icon.png");
    assert!(specialize(&input, &input, SPEC, &icon, "Twitter", "1.0.0").is_err());
    let _ = std::fs::remove_dir_all(temp);
}

const SPEC: &[u8] = br#"{"schemaVersion":1,"appId":"a123","displayName":"Photopea","slug":"photopea","launchUrl":"https://www.photopea.com/"}"#;
