use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Files one launch or one forwarded request may open.
pub const MAX_FILES: usize = 16;
/// The largest file the bridge moves in either direction.
pub const MAX_FILE_BYTES: u64 = 4 << 30;

/// Turn a command-line argument into the canonical path of an existing file.
///
/// Canonical, so a symlink resolves to its target and saving replaces the
/// file rather than the link, and so a running instance with a different
/// current directory reads the same file. Paths over 260 characters keep the
/// `\\?\` prefix, which Win32 file calls need for them.
pub fn resolve(arg: &OsStr) -> Result<PathBuf, String> {
    let path = match dunce::canonicalize(Path::new(arg)) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("it does not exist".into());
        }
        // Some volumes (RAM disks, some virtual drives) cannot canonicalize.
        // Their files still open, without symlink resolution.
        Err(_) => std::path::absolute(Path::new(arg)).map_err(|error| error.to_string())?,
    };
    // The forwarding message is JSON, which cannot carry a non-Unicode name.
    if path.to_str().is_none() {
        return Err("its name is not valid Unicode".into());
    }
    check_file(&path)?;
    Ok(path)
}

/// Check that a path is absolute and names an existing regular file.
pub fn check_file(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("it is not an absolute path".into());
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(meta) if meta.is_dir() => Err("it is a folder, not a file".into()),
        Ok(_) => Err("it is not a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err("it does not exist".into())
        }
        Err(error) => Err(error.to_string()),
    }
}

/// The name a page is shown for a file. Never a path.
pub fn display_name(path: &Path) -> String {
    let name: String = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    if name.is_empty() {
        "file".into()
    } else {
        name
    }
}

/// Folders WebView2 does not let a site use a file handle in, read from the
/// environment: Windows keeps them for apps. Empty where the variables are
/// unset, as on Linux.
pub fn protected_folders() -> Vec<PathBuf> {
    PROTECTED_FOLDER_VARIABLES
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dunce::canonicalize(&dir).unwrap_or(dir))
        .collect()
}

const PROTECTED_FOLDER_VARIABLES: [&str; 7] = [
    "LOCALAPPDATA",
    "APPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "SystemRoot",
];

/// Whether WebView2 refuses a site's file handle for `path`: inside one of
/// the `protected` folders, or long enough to need the `\\?\` prefix. Both
/// were measured in CI (D2 evidence). Case is ignored, as Windows does.
pub fn handle_refused(path: &Path, protected: &[PathBuf]) -> bool {
    let lower = |path: &Path| PathBuf::from(path.to_string_lossy().to_lowercase());
    let path = lower(path);
    path.to_string_lossy().starts_with(r"\\?\")
        || protected
            .iter()
            .filter(|root| root.is_absolute())
            .any(|root| path.starts_with(lower(root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "webwrap-bridge-paths-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_existing_file_resolves_to_itself() {
        let dir = temp_dir("file");
        let file = dir.join("a.psd");
        std::fs::write(&file, b"8BPS").unwrap();
        let resolved = resolve(file.as_os_str()).unwrap();
        assert_eq!(resolved, dunce::canonicalize(&file).unwrap());
        // A short path stays in the plain form users and dialogs show.
        assert!(
            !resolved.to_string_lossy().starts_with(r"\\?\"),
            "{resolved:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_resolves_to_its_target_so_saving_replaces_the_file() {
        let dir = temp_dir("link");
        let target = dir.join("real.psd");
        std::fs::write(&target, b"8BPS").unwrap();
        let link = dir.join("link.psd");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            resolve(link.as_os_str()).unwrap(),
            dunce::canonicalize(&target).unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folders_missing_files_and_relative_paths_are_refused() {
        let dir = temp_dir("refused");
        assert!(resolve(dir.as_os_str()).unwrap_err().contains("folder"));
        assert!(resolve(dir.join("missing.psd").as_os_str())
            .unwrap_err()
            .contains("does not exist"));
        // A dash-prefixed argument is a file name like any other.
        assert!(resolve(OsStr::new("--inspect-selff"))
            .unwrap_err()
            .contains("does not exist"));
        assert!(check_file(Path::new("relative.psd"))
            .unwrap_err()
            .contains("absolute"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn display_names_never_carry_control_characters_or_folders() {
        assert_eq!(display_name(Path::new("/x/y/photo.psd")), "photo.psd");
        assert_eq!(display_name(Path::new("/x/a\nb.md")), "a_b.md");
        assert_eq!(display_name(Path::new("/")), "file");
    }

    #[test]
    fn handles_are_refused_inside_protected_folders_and_for_verbatim_paths() {
        let base = std::env::temp_dir();
        let protected = [
            base.join("Users/Me/AppData/Local"),
            PathBuf::from("relative"),
        ];
        let refused = |path: PathBuf| handle_refused(&path, &protected);
        assert!(refused(base.join("Users/Me/AppData/Local/Temp/a.md")));
        assert!(refused(base.join("users/me/appdata/LOCAL/a.md")));
        assert!(refused(PathBuf::from(r"\\?\C:\long\a.md")));
        assert!(!refused(base.join("Users/Me/Documents/a.md")));
        // Whole folder names only, and a relative entry protects nothing.
        assert!(!refused(base.join("Users/Me/AppData/LocalCopy/a.md")));
        assert!(!refused(base.join("relative/a.md")));
    }
}
