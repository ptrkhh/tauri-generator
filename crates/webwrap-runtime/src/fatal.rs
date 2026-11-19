use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

/// Report an unrecoverable startup failure and exit.
///
/// A release build sets `windows_subsystem = "windows"`, so it has no console
/// and `eprintln!` reaches nobody. Interactive failures therefore go through
/// a message box. Non-interactive modes keep writing to stderr so that tests
/// and pipelines can read the reason.
pub fn abort(message: &str, interactive: bool) -> ! {
    if interactive {
        let title = HSTRING::from("WebWrap");
        let body = HSTRING::from(message);
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(body.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    } else {
        eprintln!("{message}");
    }
    std::process::exit(1)
}
