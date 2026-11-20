//! Probe: what happens when the generator replaces an app that is running.
//! Windows maps a running executable's image, which may block the rename.
//! Launches a real app window, so it is run explicitly in Windows CI.
#![cfg(windows)]

mod common;

use std::process::Command;
use std::time::Duration;

use common::{png, temp_dir, template};
use webwrap_pack::{generate, AppInput, Arch, PackError, Templates};
use webwrap_spec::Adapter;

#[test]
#[ignore = "launches a real app window; run explicitly in Windows CI"]
fn regenerating_over_a_running_app() {
    let (x64, arm64) = (template(Arch::X64), template(Arch::Arm64));
    let templates = Templates {
        x64: &x64,
        arm64: &arm64,
    };
    let input = AppInput {
        url: "example.com".into(),
        display_name: "Example".into(),
        slug: None,
        adapter: Adapter::None,
        file_associations: vec![],
    };
    let dir = temp_dir("running");
    generate(&input, &png(64, [229, 57, 53, 255]), &dir, templates).unwrap();

    let native = if cfg!(target_arch = "aarch64") {
        Arch::Arm64
    } else {
        Arch::X64
    };
    let path = dir.join(format!("example-{}.exe", native.suffix()));
    let mut app = Command::new(&path).spawn().unwrap();
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        app.try_wait().unwrap().is_none(),
        "the app exited before the probe"
    );

    let before = common::snapshot(&dir);
    let result = generate(&input, &png(64, [30, 136, 229, 255]), &dir, templates);
    let still_running = app.try_wait().unwrap().is_none();
    let after = common::snapshot(&dir);
    let _ = app.kill();
    let _ = app.wait();

    // Observed in CI on 2026-11-20: Windows refuses to rename over a running
    // executable (os error 5, even with std's POSIX-semantics fallback). The
    // generator must detect that before publishing either architecture.
    assert!(still_running, "regenerating killed the running app");
    match result {
        Err(PackError::InUse { path: busy, .. }) => assert_eq!(busy, path),
        other => panic!("expected InUse for {}, got {other:?}", path.display()),
    }
    assert_eq!(before, after, "the folder changed");
}
