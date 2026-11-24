use std::fmt::Display;
use std::io::Write;

use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO,
    MESSAGEBOX_RESULT, MESSAGEBOX_STYLE,
};

fn message_box(message: &str, style: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from("WebWrap"),
            style,
        )
    }
}

/// Ask a yes/no question and wait for the answer.
pub fn ask(question: &str) -> bool {
    message_box(question, MB_YESNO | MB_ICONQUESTION) == IDYES
}

/// Show an error and wait for the user to dismiss it.
pub fn error_box(message: &str) {
    message_box(message, MB_OK | MB_ICONERROR);
}

/// Show a notice without blocking the caller.
pub fn info_box_async(message: String) {
    std::thread::spawn(move || message_box(&message, MB_OK | MB_ICONINFORMATION));
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
        // Logged first: the box blocks, and a parent watching stderr (the CI
        // end-to-end test) would otherwise see a silent, running process.
        log(format_args!("fatal {message}"));
        error_box(message);
    } else {
        eprintln!("{message}");
    }
    std::process::exit(1)
}
