//! The `photopea` adapter. The window shows a local shell page that embeds
//! Photopea and moves files between it and the app through D1's shared
//! buffers (`photopea.js`). Ported from `spikes/photopea-windows-writeback`.

use std::path::PathBuf;

use webwrap_spec::Assoc;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
    SIGDN_FILESYSPATH,
};

/// Where the shell page is served from, and so the only origin the session
/// trusts for this adapter.
pub const SHELL_ORIGIN: &str = "https://tauri.localhost";
/// The shell page, a bundled asset.
pub const SHELL_PAGE: &str = "photopea.html";

/// The shell's script, for Photopea at `url`, whose origin is `origin`.
pub fn script(url: &str, origin: &str) -> String {
    let photopea = serde_json::json!({ "url": url, "origin": origin });
    include_str!("photopea.js").replace("__WEBWRAP_PHOTOPEA__", &photopea.to_string())
}

/// The native open dialog, owned by `owner`: the app's file types first, then
/// "All files". Returns the chosen paths, or none if the user cancelled.
pub fn pick_files(owner: HWND, associations: &[Assoc]) -> windows::core::Result<Vec<PathBuf>> {
    let mut labels: Vec<(HSTRING, HSTRING)> = associations
        .iter()
        .map(|assoc| {
            (
                HSTRING::from(format!("{} (*{})", assoc.description, assoc.extension)),
                HSTRING::from(format!("*{}", assoc.extension)),
            )
        })
        .collect();
    labels.push((HSTRING::from("All files (*.*)"), HSTRING::from("*.*")));
    let filters: Vec<COMDLG_FILTERSPEC> = labels
        .iter()
        .map(|(name, spec)| COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(spec.as_ptr()),
        })
        .collect();
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let options = dialog.GetOptions()?;
        dialog
            .SetOptions(options | FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)?;
        dialog.SetFileTypes(&filters)?;
        if let Err(error) = dialog.Show(Some(owner)) {
            return if error.code() == ERROR_CANCELLED.to_hresult() {
                Ok(Vec::new())
            } else {
                Err(error)
            };
        }
        let items = dialog.GetResults()?;
        let mut paths = Vec::new();
        for index in 0..items.GetCount()? {
            let name = items.GetItemAt(index)?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = name.to_string();
            CoTaskMemFree(Some(name.0 as *const _));
            paths.push(PathBuf::from(
                path.map_err(|_| windows::core::Error::empty())?,
            ));
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_script_gets_photopeas_address_as_a_javascript_object() {
        let script = script("https://www.photopea.com/", "https://www.photopea.com");
        assert!(script.contains(
            r#"const PHOTOPEA = {"origin":"https://www.photopea.com","url":"https://www.photopea.com/"};"#
        ));
        assert!(!script.contains("__WEBWRAP_PHOTOPEA__"));
    }
}
