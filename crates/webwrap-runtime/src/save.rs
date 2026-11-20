//! Replacing a user's file without ever leaving it half-written.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use windows::core::HSTRING;
use windows::Win32::Foundation::ERROR_UNABLE_TO_MOVE_REPLACEMENT_2;
use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_IGNORE_MERGE_ERRORS};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A same-folder name that does not exist yet: `.webwrap-<pid>-<n>.<ext>`.
fn unused_name(dir: &Path, ext: &str) -> PathBuf {
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!(".webwrap-{}-{n}.{ext}", std::process::id()));
        if !path.exists() {
            return path;
        }
    }
}

fn create_temp(dir: &Path) -> io::Result<(PathBuf, File)> {
    loop {
        let path = unused_name(dir, "tmp");
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Replace `path`'s content with `bytes`.
///
/// The bytes go to a same-folder temporary file, which is flushed to disk and
/// then swapped in by `ReplaceFileW`. Unlike a rename, that keeps the
/// original's creation time, ACLs, attributes, and alternate streams. On any
/// failure the original is left as it was. A missing original is an error:
/// the user's file is never recreated somewhere it was deleted from.
pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the file has no folder"))?;
    let unchanged = |error: io::Error| {
        io::Error::new(
            error.kind(),
            format!("{error}. The original file was not changed."),
        )
    };
    let (temp, mut file) = create_temp(dir).map_err(unchanged)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = written
        .map_err(unchanged)
        .and_then(|()| swap_in(path, &temp, dir));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// The `\\?\` form of an absolute path, which `ReplaceFileW` needs once a
/// path passes 260 characters; the temporary and backup names can push a
/// path just under the limit over it. std's own file calls add it themselves.
fn verbatim(path: &Path) -> HSTRING {
    let text = path.as_os_str().to_string_lossy();
    if text.starts_with(r"\\?\") {
        HSTRING::from(path.as_os_str())
    } else if let Some(share) = text.strip_prefix(r"\\") {
        HSTRING::from(format!(r"\\?\UNC\{share}"))
    } else {
        HSTRING::from(format!(r"\\?\{text}"))
    }
}

fn swap_in(path: &Path, temp: &Path, dir: &Path) -> io::Result<()> {
    // Without a backup name, ERROR_UNABLE_TO_MOVE_REPLACEMENT leaves no file
    // at the original name at all. With one, the original keeps its name.
    let backup = unused_name(dir, "bak");
    let replaced = unsafe {
        ReplaceFileW(
            &verbatim(path),
            &verbatim(temp),
            &verbatim(backup.as_path()),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            None,
            None,
        )
    };
    match replaced {
        Ok(()) => {
            let _ = fs::remove_file(&backup);
            Ok(())
        }
        Err(error) if error.code() == ERROR_UNABLE_TO_MOVE_REPLACEMENT_2.to_hresult() => {
            // The original was moved to the backup name and the new content
            // did not take its place. Put the original back.
            fs::rename(&backup, path).map_err(|undo| {
                io::Error::other(format!(
                    "{error}. Putting the original back also failed ({undo}): it is now named {}",
                    backup.display()
                ))
            })?;
            Err(io::Error::other(format!(
                "{error}. The original file was not changed."
            )))
        }
        Err(error) => Err(io::Error::other(format!(
            "{error}. The original file was not changed."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("webwrap-save-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn under_wine() -> bool {
        use windows::core::{s, w};
        use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        unsafe {
            GetModuleHandleW(w!("ntdll.dll"))
                .is_ok_and(|ntdll| GetProcAddress(ntdll, s!("wine_get_version")).is_some())
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn content_is_replaced_and_the_creation_time_kept() {
        let dir = temp_dir("ok");
        let path = dir.join("a.psd");
        fs::write(&path, b"8BPS old").unwrap();
        let created = fs::metadata(&path).unwrap().created().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        replace(&path, b"8BPS new and longer").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"8BPS new and longer");
        if under_wine() {
            eprintln!(
                "note: Wine's ReplaceFileW does not keep creation times; Windows CI checks it"
            );
        } else {
            assert_eq!(fs::metadata(&path).unwrap().created().unwrap(), created);
        }
        assert_eq!(names(&dir), ["a.psd"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_read_only_original_is_untouched_and_nothing_is_left_behind() {
        let dir = temp_dir("readonly");
        let path = dir.join("a.psd");
        fs::write(&path, b"8BPS old").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions.clone()).unwrap();
        let error = replace(&path, b"8BPS new").unwrap_err().to_string();
        assert!(error.contains("not changed"), "{error}");
        assert_eq!(fs::read(&path).unwrap(), b"8BPS old");
        assert_eq!(names(&dir), ["a.psd"]);
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_path_near_the_length_limit_still_saves() {
        // The folder brings the file to about 250 characters, so the
        // temporary and backup names next to it pass 260.
        let mut dir = temp_dir("long");
        while dir.as_os_str().len() < 240 {
            dir.push("d".repeat(20));
        }
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.psd");
        fs::write(&path, b"8BPS old").unwrap();
        replace(&path, b"8BPS new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"8BPS new");
        assert_eq!(names(&dir), ["a.psd"]);
        fs::remove_dir_all(temp_dir("long")).unwrap();
    }

    #[test]
    fn a_missing_original_fails_and_is_not_recreated() {
        let dir = temp_dir("missing");
        let path = dir.join("gone.psd");
        assert!(replace(&path, b"8BPS new").is_err());
        assert!(names(&dir).is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
