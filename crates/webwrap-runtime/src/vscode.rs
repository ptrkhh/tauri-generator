//! The `vscodeDev` adapter. A file opens as a read-write handle that
//! vscode.dev's own "Open File..." command asks for, so vscode.dev's Ctrl+S
//! writes the original file. The runtime only posts the handle; the page
//! script in `vscode.js` runs the command.

use std::path::Path;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment14, ICoreWebView2ObjectCollection, ICoreWebView2ObjectCollectionView,
    ICoreWebView2_23, COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
};
use windows::core::{IUnknown, Interface, HSTRING};

/// The adapter's page script for an app that trusts `origins`.
pub fn script(origins: &[String]) -> String {
    let origins = serde_json::to_string(origins).expect("strings always serialize");
    include_str!("vscode.js").replace("__WEBWRAP_TRUSTED__", &origins)
}

/// Post `message` with a read-write handle to `path` as its one additional object.
pub fn post_handle(
    environment: &ICoreWebView2Environment14,
    webview: &ICoreWebView2_23,
    path: &Path,
    message: &str,
) -> windows::core::Result<()> {
    unsafe {
        let handle = environment.CreateWebFileSystemFileHandle(
            &HSTRING::from(path),
            COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
        )?;
        let mut items = [Some(handle.cast::<IUnknown>()?)];
        let mut collection: Option<ICoreWebView2ObjectCollection> = None;
        environment.CreateObjectCollection(1, items.as_mut_ptr(), &mut collection)?;
        let collection = collection
            .ok_or_else(windows::core::Error::empty)?
            .cast::<ICoreWebView2ObjectCollectionView>()?;
        webview.PostWebMessageAsJsonWithAdditionalObjects(&HSTRING::from(message), &collection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_gets_the_origins_as_a_javascript_array() {
        let script = script(&["https://vscode.dev".into()]);
        assert!(script.contains(r#"const trusted = ["https://vscode.dev"];"#));
        assert!(!script.contains("__WEBWRAP_TRUSTED__"));
    }
}
