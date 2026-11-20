//! Probe: what happens when the generator replaces an app that is running.
//! Windows maps a running executable's image, which may block the rename.
//! Launches a real app window, so it is run explicitly in Windows CI.
#![cfg(windows)]

mod common;

use std::fs;
use std::process::Command;
use std::time::Duration;

use common::{png, temp_dir, template};
use webwrap_pack::{generate, inspect, AppInput, Arch, Templates};
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

    let before = fs::read(&path).unwrap();
    let result = generate(&input, &png(64, [30, 136, 229, 255]), &dir, templates);
    let still_running = app.try_wait().unwrap().is_none();
    let after = fs::read(&path).unwrap();
    let _ = app.kill();
    let _ = app.wait();

    println!(
        "PROBE native={} result={:?} still_running={still_running} replaced={}",
        native.suffix(),
        result.as_ref().map(|_| ()).map_err(ToString::to_string),
        before != after
    );
    // Whatever Windows decides, the file at the path is a complete app and
    // the running one was not disturbed.
    assert!(still_running, "regenerating killed the running app");
    assert!(inspect(&after).unwrap().spec.is_some());
    match result {
        Ok(_) => assert_ne!(before, after, "reported success but nothing changed"),
        Err(_) => assert_eq!(before, after, "failed but the running app's file changed"),
    }
}
