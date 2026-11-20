//! `check_icon` must not inflate an ICC profile beyond its allocation bound.
//! Its own test binary, because it installs a global allocator that records
//! the largest single allocation.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use webwrap_pack::{check_icon, spec::MAX_ICON_BYTES};

struct Peak;
static LARGEST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LARGEST.fetch_max(new_size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Peak = Peak;

#[test]
fn a_huge_colour_profile_is_not_inflated_past_the_check_bound() {
    // 64 MiB of zeros compresses to well under the 10 MiB file limit.
    let mut png = Vec::new();
    let mut encoder = PngEncoder::new(&mut png);
    encoder.set_icc_profile(vec![0; 64 * 1024 * 1024]).unwrap();
    encoder
        .write_image(&[0; 64 * 64 * 4], 64, 64, ExtendedColorType::Rgba8)
        .unwrap();
    assert!(png.len() < MAX_ICON_BYTES, "fixture is {} bytes", png.len());

    LARGEST.store(0, Ordering::Relaxed);
    check_icon(&png).unwrap(); // the profile is dropped, not an error
    let largest = LARGEST.load(Ordering::Relaxed) as u64;
    // A fixed bar, not ICON_CHECK_MAX_ALLOC, so loosening the constant
    // fails here. Measured 2026-11-20: 8.6 MB bounded, 69 MB unbounded.
    assert!(
        largest < 16 * 1024 * 1024,
        "largest allocation was {largest} bytes, so the 64 MiB profile was inflated"
    );
}
