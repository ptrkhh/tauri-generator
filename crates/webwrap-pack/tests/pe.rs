mod common;

use common::{png, template};
use webwrap_pack::{inspect, machine_of, specialize_raw, Arch, PackError};

const SPEC: &[u8] = br#"{"any":"bytes, specialize_raw does not validate"}"#;

#[test]
fn templates_are_unspecialized_and_of_their_architecture() {
    for arch in Arch::ALL {
        let report = inspect(&template(arch)).unwrap();
        assert_eq!(report.arch, Some(arch));
        assert_eq!(
            report.spec, None,
            "{arch:?} template already carries a spec"
        );
        assert!(
            report.icon_present,
            "{arch:?} template has no placeholder icon"
        );
    }
}

#[test]
fn specialize_raw_round_trips_on_both_architectures() {
    let icon = png(64, [229, 57, 53, 255]);
    for arch in Arch::ALL {
        let template = template(arch);
        let app = specialize_raw(&template, SPEC, &icon, "Photopea", "1.2.3").unwrap();
        assert_eq!(machine_of(&app), Some(arch.machine()));
        let report = inspect(&app).unwrap();
        assert_eq!(report.arch, Some(arch));
        assert_eq!(report.spec.as_deref(), Some(SPEC));
        assert!(report.icon_present);
        assert_eq!(report.file_description.as_deref(), Some("Photopea"));
        assert_eq!(report.product_version.as_deref(), Some("1.2.3"));
    }
}

#[test]
fn respecializing_replaces_rather_than_accumulates() {
    let template = template(Arch::X64);
    let red = png(64, [229, 57, 53, 255]);
    let blue = png(64, [30, 136, 229, 255]);
    let first = specialize_raw(&template, b"first", &red, "First", "1.0.0").unwrap();
    let second = specialize_raw(&first, b"second", &blue, "Second", "2.0.0").unwrap();
    let report = inspect(&second).unwrap();
    assert_eq!(report.spec.as_deref(), Some(&b"second"[..]));
    assert_eq!(report.file_description.as_deref(), Some("Second"));
    assert_eq!(report.product_version.as_deref(), Some("2.0.0"));
}

#[test]
fn bad_inputs_are_errors_not_panics() {
    let template = template(Arch::X64);
    assert!(matches!(
        specialize_raw(&template, SPEC, b"not a png", "A", "1.0.0"),
        Err(PackError::Input { field: "icon", .. })
    ));
    let icon = png(16, [0, 0, 0, 255]);
    assert!(matches!(
        specialize_raw(b"not a pe", SPEC, &icon, "A", "1.0.0"),
        Err(PackError::Pe(_))
    ));
    assert!(inspect(b"not a pe").is_err());
}
