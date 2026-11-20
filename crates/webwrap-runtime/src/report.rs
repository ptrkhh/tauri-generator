use std::fmt::Display;
use std::io::Write;

use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

/// Show an error and wait for the user to dismiss it.
pub fn error_box(message: &str) {
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from("WebWrap"),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Show an error without blocking the caller, so the window keeps working.
pub fn error_box_async(message: String) {
    std::thread::spawn(move || error_box(&message));
}

/// One diagnostic line on stderr. A release build has no console, so this
/// reaches only a parent that captured stderr, such as the CI end-to-end test.
/// Write errors are ignored: `eprintln!` would panic on a closed pipe.
pub fn log(message: impl Display) {
    let _ = writeln!(std::io::stderr(), "webwrap: {message}");
}

/// Report an unrecoverable startup failure and exit.
///
/// A release build sets `windows_subsystem = "windows"`, so it has no console
/// and `eprintln!` reaches nobody. Interactive failures therefore go through
/// a message box. Non-interactive modes keep writing to stderr so that tests
/// and pipelines can read the reason.
pub fn abort(message: &str, interactive: bool) -> ! {
    if interactive {
        error_box(message);
    } else {
        eprintln!("{message}");
    }
    std::process::exit(1)
}
