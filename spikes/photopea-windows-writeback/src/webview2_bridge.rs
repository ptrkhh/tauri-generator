use tauri::WebviewWindow;

use crate::LaunchRequest;

pub fn install(_window: &WebviewWindow, _request: LaunchRequest) -> tauri::Result<()> {
    Ok(())
}
