#![allow(dead_code)] // each test file uses a different subset

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use image::{ImageFormat, Rgba, RgbaImage};
use webwrap_pack::Arch;

/// The runtime template for an architecture. Fails rather than skips when
/// unset, so CI cannot pass without exercising real templates.
pub fn template(arch: Arch) -> Vec<u8> {
    let var = match arch {
        Arch::X64 => "WEBWRAP_TEMPLATE_X64",
        Arch::Arm64 => "WEBWRAP_TEMPLATE_ARM64",
    };
    let path = std::env::var_os(var).unwrap_or_else(|| {
        panic!(
            "{var} is not set: build webwrap-runtime for both Windows targets and point \
             WEBWRAP_TEMPLATE_X64 and WEBWRAP_TEMPLATE_ARM64 at the two webwrap-runtime.exe files"
        )
    });
    std::fs::read(&path).unwrap_or_else(|error| panic!("{var}={path:?}: {error}"))
}

/// A square solid-colour PNG.
pub fn png(side: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    RgbaImage::from_pixel(side, side, Rgba(rgba))
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

pub fn temp_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "webwrap-pack-{label}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
