//! "Open with" registration for the current user (sub-project E).
//!
//! Writes only this app's own ProgID tree and its own value in each
//! extension's `OpenWithProgids`, under `HKCU\Software\Classes`. Never
//! `UserChoice` or anything else in Explorer's `FileExts`: Windows, and the
//! user through Windows' own prompt, decide the default app.

use std::path::{Path, PathBuf};

use webwrap_spec::Assoc;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
    RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

use crate::report;

/// What one app registers. Names come from the validated spec: the app id is
/// hex and extensions are a dot plus lowercase letters and digits, so no key
/// or value name can contain `\` or a quote.
pub struct Registration<'a> {
    pub prog_id: String,
    pub display_name: &'a str,
    pub extensions: Vec<&'a str>,
    pub exe: PathBuf,
}

impl<'a> Registration<'a> {
    pub fn new(app_id: &str, display_name: &'a str, assocs: &'a [Assoc], exe: PathBuf) -> Self {
        Self {
            prog_id: format!("webwrap.{app_id}"),
            display_name,
            extensions: assocs
                .iter()
                .map(|assoc| assoc.extension.as_str())
                .collect(),
            exe,
        }
    }

    fn key(&self, sub: &str) -> String {
        format!(r"Software\Classes\{}{sub}", self.prog_id)
    }

    // shortcut: a path that is not valid UTF-16 is shown lossily, so it never
    // reads back equal and the offer repeats; such paths are not seen in practice.
    fn command(&self) -> String {
        format!("\"{}\" \"%1\"", self.exe.display())
    }

    /// Write the ProgID and add it to each extension's Open with list.
    /// Rewriting is harmless, so this also repairs a partial registration.
    pub fn register(&self) -> Result<(), String> {
        let exe = self.exe.display();
        set(&self.key(""), None, self.display_name)?;
        set(&self.key(r"\DefaultIcon"), None, &format!("\"{exe}\",0"))?;
        set(
            &self.key(r"\Application"),
            Some("ApplicationName"),
            self.display_name,
        )?;
        set(&self.key(r"\shell\open\command"), None, &self.command())?;
        for ext in &self.extensions {
            set(&open_with(ext), Some(&self.prog_id), "")?;
        }
        notify_explorer();
        Ok(())
    }

    /// Remove exactly what `register` wrote: the ProgID tree and this app's
    /// own value per extension, never the `OpenWithProgids` key or another
    /// app's value. Missing pieces are fine.
    pub fn unregister(&self) -> Result<(), String> {
        let tree = self.key("");
        absent_ok(
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(&tree)) },
            &tree,
        )?;
        for ext in &self.extensions {
            let key = open_with(ext);
            let deleted = unsafe {
                RegDeleteKeyValueW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(&key),
                    &HSTRING::from(&self.prog_id),
                )
            };
            absent_ok(deleted, &key)?;
        }
        notify_explorer();
        Ok(())
    }

    /// Whether this exe is the registered command and every extension lists
    /// the ProgID. The path compares case-insensitively, as Windows does.
    pub fn registered_here(&self) -> bool {
        let Some(command) = get(&self.key(r"\shell\open\command"), None) else {
            return false;
        };
        let expected: Vec<u16> = self.command().encode_utf16().collect();
        let actual: Vec<u16> = command.encode_utf16().collect();
        if unsafe { CompareStringOrdinal(&actual, &expected, true) } != CSTR_EQUAL {
            return false;
        }
        self.extensions
            .iter()
            .all(|ext| get(&open_with(ext), Some(&self.prog_id)).is_some())
    }
}

fn open_with(ext: &str) -> String {
    format!(r"Software\Classes\{ext}\OpenWithProgids")
}

fn name_ptr(name: Option<&HSTRING>) -> PCWSTR {
    name.map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr()))
}

/// Set a REG_SZ value, creating the key if needed.
fn set(key: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let name = name.map(HSTRING::from);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            name_ptr(name.as_ref()),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        )
    }
    .ok()
    .map_err(|error| format!(r"HKCU\{key} could not be written: {}", error.message()))
}

/// A REG_SZ value, or None if it is missing or unreadable.
fn get(key: &str, name: Option<&str>) -> Option<String> {
    let key = HSTRING::from(key);
    let name = name.map(HSTRING::from);
    let mut buffer = vec![0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            name_ptr(name.as_ref()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    }
    .ok()
    .ok()?;
    // `bytes` counts the terminating null.
    buffer.truncate((bytes as usize / 2).saturating_sub(1));
    String::from_utf16(&buffer).ok()
}

fn absent_ok(result: WIN32_ERROR, key: &str) -> Result<(), String> {
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    result
        .ok()
        .map_err(|error| format!(r"HKCU\{key} could not be removed: {}", error.message()))
}

/// Explorer caches associations; this makes it read them again now.
fn notify_explorer() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

/// This exe's own path, which is what gets registered.
pub fn this_exe() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|error| format!("this app could not find its own path: {error}"))
}

/// `--register-associations` / `--unregister-associations`: act, print one
/// line, and exit.
pub fn run_flag(app_id: &str, display_name: &str, assocs: &[Assoc], register: bool) -> ! {
    if assocs.is_empty() {
        println!("this app has no file associations");
        std::process::exit(0);
    }
    let exe = this_exe().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1)
    });
    let registration = Registration::new(app_id, display_name, assocs, exe);
    let result = if register {
        registration.register()
    } else {
        registration.unregister()
    };
    match result {
        Ok(()) => {
            let done = if register {
                "registered"
            } else {
                "unregistered"
            };
            println!(
                "{done} {} for {}",
                registration.prog_id,
                registration.extensions.join(" ")
            );
            std::process::exit(0)
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}

/// The first-launch offer. Asks only when the app has associations, this exe
/// is not the registered one, and the user has not declined before. Blocks
/// until answered; runs before the window, after the single-instance pipe is
/// served, so a launch arriving meanwhile is queued rather than timed out.
pub fn offer(app_id: &str, display_name: &str, assocs: &[Assoc], data_dir: &Path) {
    if assocs.is_empty() {
        return;
    }
    let Ok(exe) = this_exe() else { return };
    let registration = Registration::new(app_id, display_name, assocs, exe);
    if registration.registered_here() {
        return;
    }
    let declined = data_dir.join("assoc-declined");
    if declined.exists() {
        report::log("assoc: stale");
        return;
    }
    let list = registration.extensions.join(", ");
    let question = format!(
        "Add {display_name} to \"Open with\" for {list} files?\n\n\
         This only affects your own account, and does not change any default app."
    );
    if report::ask(&question) {
        match registration.register() {
            Ok(()) => report::log("assoc: registered"),
            Err(error) => {
                report::log(format_args!("assoc: failed {error}"));
                report::error_box_async(format!(
                    "{display_name} could not be added to \"Open with\": {error}"
                ));
            }
        }
    } else {
        // If the flag cannot be written, the offer simply comes back next time.
        let _ = std::fs::create_dir_all(data_dir);
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&declined);
        report::log("assoc: declined");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assoc(ext: &str) -> Assoc {
        Assoc {
            extension: ext.into(),
            mime_type: "application/x-webwrap-test".into(),
            description: "test".into(),
        }
    }

    #[test]
    fn register_and_unregister_touch_only_this_apps_keys() {
        // A test-only id and extension, so no real app's keys are touched.
        let id = format!("test{}", std::process::id());
        let ext = format!(".wwtest{}", std::process::id());
        let assocs = [assoc(&ext)];
        let exe = PathBuf::from(r"C:\Apps\Test App\test-x64.exe");
        let reg = Registration::new(&id, "Test App", &assocs, exe);
        let list = open_with(&ext);
        // Foreign state that must survive: another app's Open with value and
        // a UserChoice key the user set through Windows.
        let user_choice = format!(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{ext}\UserChoice"
        );
        set(&list, Some("Other.App"), "").unwrap();
        set(&user_choice, Some("ProgId"), "Other.App").unwrap();

        assert!(!reg.registered_here());
        reg.register().unwrap();
        assert_eq!(
            get(&reg.key(r"\shell\open\command"), None).unwrap(),
            r#""C:\Apps\Test App\test-x64.exe" "%1""#
        );
        assert_eq!(get(&reg.key(""), None).unwrap(), "Test App");
        assert!(reg.registered_here());

        // Same exe through a differently-cased path: still registered here.
        let upper = Registration::new(
            &id,
            "Test App",
            &assocs,
            r"C:\APPS\Test App\TEST-x64.exe".into(),
        );
        assert!(upper.registered_here());
        // A moved exe is not.
        let moved = Registration::new(&id, "Test App", &assocs, r"D:\Moved\test-x64.exe".into());
        assert!(!moved.registered_here());

        reg.unregister().unwrap();
        assert!(!reg.registered_here());
        assert!(get(&reg.key(r"\shell\open\command"), None).is_none());
        assert!(get(&list, Some(&reg.prog_id)).is_none());
        assert_eq!(get(&list, Some("Other.App")).as_deref(), Some(""));
        assert_eq!(
            get(&user_choice, Some("ProgId")).as_deref(),
            Some("Other.App")
        );
        // A second unregister finds nothing and is fine.
        reg.unregister().unwrap();

        for key in [
            format!(r"Software\Classes\{ext}"),
            format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{ext}"),
        ] {
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(&key)) };
        }
    }

    #[test]
    fn a_missing_extension_value_means_not_registered_here() {
        let id = format!("test{}b", std::process::id());
        let ext = format!(".wwtestb{}", std::process::id());
        let assocs = [assoc(&ext)];
        let reg = Registration::new(&id, "Test App", &assocs, r"C:\Apps\t.exe".into());
        reg.register().unwrap();
        let deleted = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(open_with(&ext)),
                &HSTRING::from(&reg.prog_id),
            )
        };
        assert!(deleted.is_ok());
        assert!(!reg.registered_here());
        reg.unregister().unwrap();
        let _ = unsafe {
            RegDeleteTreeW(
                HKEY_CURRENT_USER,
                &HSTRING::from(format!(r"Software\Classes\{ext}")),
            )
        };
    }
}
