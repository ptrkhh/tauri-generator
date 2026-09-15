use std::path::PathBuf;

use editpe::Image;
use windows_pe_specialization_spike::pe::{edit_resources, read_metadata};

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("webwrap-pe-test-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
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
