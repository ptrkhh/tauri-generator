mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{png, snapshot, temp_dir, template};
use webwrap_pack::{
    build_spec, generate, inspect, AppInput, Arch, Generated, PackError, Replaced, Templates,
};
use webwrap_spec::{Adapter, AppSpec};

const RED: [u8; 4] = [229, 57, 53, 255];
const BLUE: [u8; 4] = [30, 136, 229, 255];

fn input(url: &str, name: &str) -> AppInput {
    AppInput {
        url: url.into(),
        display_name: name.into(),
        slug: None,
        adapter: Adapter::None,
        file_associations: vec![],
    }
}

struct Fixture {
    x64: Vec<u8>,
    arm64: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            x64: template(Arch::X64),
            arm64: template(Arch::Arm64),
        }
    }

    fn templates(&self) -> Templates<'_> {
        Templates {
            x64: &self.x64,
            arm64: &self.arm64,
        }
    }

    fn generate(
        &self,
        input: &AppInput,
        colour: [u8; 4],
        dir: &Path,
    ) -> Result<Generated, PackError> {
        generate(input, &png(64, colour), dir, self.templates())
    }
}

fn names(dir: &Path) -> Vec<String> {
    snapshot(dir).into_keys().collect()
}

#[test]
fn gate_1_an_empty_folder_gets_exactly_two_single_file_apps() {
    let fixture = Fixture::new();
    let dir = temp_dir("gate1");
    let twitter = input("twitter.com", "Twitter");
    let generated = fixture.generate(&twitter, RED, &dir).unwrap();

    assert_eq!(generated.slug, "twitter");
    assert_eq!(generated.app_id, "2fa5e3f40150278f");
    assert_eq!(names(&dir), ["twitter-arm64.exe", "twitter-x64.exe"]);
    let (_, spec_bytes) = build_spec(&twitter).unwrap();
    for output in &generated.outputs {
        assert_eq!(output.replaced, Replaced::None);
        let report = inspect(&fs::read(&output.path).unwrap()).unwrap();
        assert_eq!(report.arch, Some(output.arch));
        assert_eq!(report.spec.as_deref(), Some(&spec_bytes[..]));
        let spec = AppSpec::from_slice(report.spec.as_deref().unwrap()).unwrap();
        assert_eq!(spec.app_id, generated.app_id);
        assert!(report.icon_present);
        assert_eq!(report.file_description.as_deref(), Some("Twitter"));
        assert_eq!(report.product_version.as_deref(), Some("1.0.0"));
    }
    assert_eq!(
        generated
            .outputs
            .iter()
            .map(|o| o.path.clone())
            .collect::<Vec<_>>(),
        [dir.join("twitter-x64.exe"), dir.join("twitter-arm64.exe")]
    );
}

#[test]
fn gate_3_regenerating_twitter_updates_only_twitter() {
    let fixture = Fixture::new();
    let dir = temp_dir("gate3");
    fixture
        .generate(&input("facebook.com", "Facebook"), BLUE, &dir)
        .unwrap();
    fixture
        .generate(&input("twitter.com", "Twitter"), RED, &dir)
        .unwrap();
    let before = snapshot(&dir);

    let again = fixture
        .generate(&input("twitter.com", "Twitter"), BLUE, &dir)
        .unwrap();
    assert!(again
        .outputs
        .iter()
        .all(|o| o.replaced == Replaced::SameApp));
    let after = snapshot(&dir);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for name in ["facebook-x64.exe", "facebook-arm64.exe"] {
        assert_eq!(before[name], after[name], "{name} changed");
    }
    for name in ["twitter-x64.exe", "twitter-arm64.exe"] {
        assert_ne!(before[name], after[name], "{name} was not regenerated");
    }
}

#[test]
fn a_different_site_with_the_same_name_replaces_the_old_app() {
    let fixture = Fixture::new();
    let dir = temp_dir("other");
    fixture
        .generate(&input("twitter.com", "Twitter"), RED, &dir)
        .unwrap();
    let other = fixture
        .generate(&input("twitter.com/home", "Twitter Home"), RED, &dir)
        .unwrap();

    assert!(other
        .outputs
        .iter()
        .all(|o| o.replaced == Replaced::OtherApp));
    assert_ne!(other.app_id, "2fa5e3f40150278f");
    for output in &other.outputs {
        let spec = inspect(&fs::read(&output.path).unwrap())
            .unwrap()
            .spec
            .unwrap();
        assert_eq!(AppSpec::from_slice(&spec).unwrap().app_id, other.app_id);
    }
}

#[test]
fn a_foreign_file_blocks_both_architectures() {
    let fixture = Fixture::new();
    for (label, foreign) in [
        ("text", b"someone else's file".to_vec()),
        // A real executable that is not a generated app.
        ("pe", template(Arch::X64)),
    ] {
        let dir = temp_dir(label);
        let target: PathBuf = dir.join("twitter-arm64.exe");
        fs::write(&target, &foreign).unwrap();
        let before = snapshot(&dir);

        let error = fixture
            .generate(&input("twitter.com", "Twitter"), RED, &dir)
            .unwrap_err();
        assert!(
            matches!(&error, PackError::Foreign(path) if *path == target),
            "{label}: {error}"
        );
        assert_eq!(snapshot(&dir), before, "{label}: folder changed");
    }
}

#[test]
fn a_directory_at_the_target_is_refused() {
    let fixture = Fixture::new();
    let dir = temp_dir("dir");
    fs::create_dir(dir.join("twitter-x64.exe")).unwrap();
    let error = fixture
        .generate(&input("twitter.com", "Twitter"), RED, &dir)
        .unwrap_err();
    assert!(matches!(error, PackError::Foreign(_)), "{error}");
    assert_eq!(names(&dir), ["twitter-x64.exe"]);
}

#[test]
fn failures_leave_the_folder_exactly_as_it_was() {
    let fixture = Fixture::new();
    let dir = temp_dir("fail");
    fixture
        .generate(&input("twitter.com", "Twitter"), RED, &dir)
        .unwrap();
    let before = snapshot(&dir);
    let twitter = input("twitter.com", "Twitter");

    let not_square = {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(64, 32)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    };
    let cases: Vec<(&str, Result<Generated, PackError>)> = vec![
        (
            "non-square icon",
            generate(&twitter, &not_square, &dir, fixture.templates()),
        ),
        (
            "empty name",
            fixture.generate(&input("twitter.com", ""), BLUE, &dir),
        ),
        (
            "http url",
            fixture.generate(&input("http://twitter.com/", "Twitter"), BLUE, &dir),
        ),
        (
            "swapped templates",
            generate(
                &twitter,
                &png(64, BLUE),
                &dir,
                Templates {
                    x64: &fixture.arm64,
                    arm64: &fixture.x64,
                },
            ),
        ),
    ];
    for (label, result) in cases {
        assert!(result.is_err(), "{label} succeeded");
        assert_eq!(snapshot(&dir), before, "{label} changed the folder");
    }
}

#[test]
fn a_missing_output_folder_is_an_input_error() {
    let fixture = Fixture::new();
    let missing = temp_dir("missing").join("does-not-exist");
    let error = fixture
        .generate(&input("twitter.com", "Twitter"), RED, &missing)
        .unwrap_err();
    assert!(
        matches!(
            error,
            PackError::Input {
                field: "output folder",
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn a_read_only_generated_app_is_refused_before_anything_is_published() {
    let fixture = Fixture::new();
    let dir = temp_dir("readonly");
    fixture
        .generate(&input("twitter.com", "Twitter"), RED, &dir)
        .unwrap();
    let target = dir.join("twitter-arm64.exe");
    let mut permissions = fs::metadata(&target).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&target, permissions.clone()).unwrap();
    let before = snapshot(&dir);

    let error = fixture
        .generate(&input("twitter.com", "Twitter"), BLUE, &dir)
        .unwrap_err();
    assert!(
        matches!(&error, PackError::Io { path, .. } if *path == target),
        "{error}"
    );
    assert_eq!(snapshot(&dir), before, "the x64 app was published anyway");

    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&target, permissions).unwrap();
}

#[test]
fn a_file_that_appears_during_generation_is_never_replaced() {
    let fixture = Fixture::new();
    let dir = temp_dir("race");
    let target = dir.join("twitter-x64.exe");
    let foreign = b"a download that finished mid-generation".to_vec();

    // Write the foreign file the moment staging begins, which is after the
    // first check of the targets and before publishing.
    let watcher = {
        let (dir, target, foreign) = (dir.clone(), target.clone(), foreign.clone());
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while std::time::Instant::now() < deadline {
                let staging = fs::read_dir(&dir).unwrap().any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".webwrap-")
                });
                if staging {
                    fs::write(&target, &foreign).unwrap();
                    return true;
                }
            }
            false
        })
    };
    let result = fixture.generate(&input("twitter.com", "Twitter"), RED, &dir);
    assert!(watcher.join().unwrap(), "staging never started");

    // Either generation saw the file and refused, or the file arrived after
    // publishing; in both cases it must still be intact.
    assert_eq!(fs::read(&target).unwrap(), foreign, "result was {result:?}");
    assert!(!names(&dir).iter().any(|name| name.starts_with(".webwrap-")));
}
