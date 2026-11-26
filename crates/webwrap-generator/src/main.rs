//! `tauri_generator`: the window that turns a website into its two Windows
//! apps (sub-project F). Discovery is `webwrap-discover`, generation is
//! `webwrap_pack::generate`; this file is the window and four commands.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("webwrap-generator must be built and run on Windows");

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::webview::NewWindowResponse;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use webwrap_discover::Discovery;
use webwrap_pack::{AppInput, Templates};
use webwrap_spec::{Adapter, Assoc};
use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_FORCEFILESYSTEM,
    FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

const TEMPLATE_X64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/template-x64.exe"));
const TEMPLATE_ARM64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/template-arm64.exe"));

/// The bundled page's origin, the only one the window may show.
const ORIGIN: &str = "https://tauri.localhost";

/// wry's own WebView2 arguments, which `additional_browser_args` replaces;
/// the same list as the runtime's `WRY_BROWSER_ARGS`.
const WRY_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
     --autoplay-policy=no-user-gesture-required";

/// `%LOCALAPPDATA%\WebWrap\generator`: WebView2 data and the last folder.
/// "generator" is not hex, so it can never be an app's folder.
fn data_dir() -> Result<PathBuf, String> {
    match std::env::var("LOCALAPPDATA") {
        Ok(local) if !local.is_empty() => {
            Ok(PathBuf::from(local).join("WebWrap").join("generator"))
        }
        _ => Err("LOCALAPPDATA is not set, so the generator has nowhere to keep its data".into()),
    }
}

/// Run `work` off the main thread: blocking reqwest panics inside an async
/// task, and a sync command would freeze the window.
async fn off_main<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn discover(url: String) -> Result<Discovery, String> {
    off_main(move || {
        let client = webwrap_discover::client(None)?;
        webwrap_discover::discover(&client, &url)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    url: String,
    name: String,
    slug: String,
    adapter: Adapter,
    associations: Vec<Assoc>,
    icon_png: Vec<u8>,
    out_dir: PathBuf,
}

#[derive(Serialize)]
struct Written {
    path: String,
    replaced: &'static str,
}

#[tauri::command]
async fn generate(request: Request) -> Result<Vec<Written>, String> {
    off_main(move || {
        if TEMPLATE_X64.is_empty() || TEMPLATE_ARM64.is_empty() {
            return Err("this generator was built without its app templates".into());
        }
        let input = AppInput {
            url: request.url,
            display_name: request.name,
            // Always the page's slug, from the host the user typed; pack
            // would otherwise derive it from the wrapped URL.
            slug: Some(request.slug),
            adapter: request.adapter,
            file_associations: request.associations,
        };
        let templates = Templates {
            x64: TEMPLATE_X64,
            arm64: TEMPLATE_ARM64,
        };
        let generated =
            webwrap_pack::generate(&input, &request.icon_png, &request.out_dir, templates)
                .map_err(|error| error.to_string())?;
        Ok(generated
            .outputs
            .into_iter()
            .map(|output| Written {
                path: output.path.display().to_string(),
                replaced: output.replaced.as_str(),
            })
            .collect())
    })
    .await
}

/// The Windows folder picker, opening at the last folder chosen. A sync
/// command, so it runs on the main thread, where the window's COM lives.
#[tauri::command]
fn pick_folder(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let owner = window.hwnd().map_err(|error| error.to_string())?;
    let remembered = data_dir().ok().map(|dir| dir.join("last-folder"));
    let start = remembered
        .as_ref()
        .and_then(|file| std::fs::read_to_string(file).ok());
    let picked = pick(owner, start.as_deref()).map_err(|error| error.message())?;
    if let (Some(folder), Some(file)) = (&picked, &remembered) {
        let _ = std::fs::write(file, folder);
    }
    Ok(picked)
}

fn pick(owner: HWND, start: Option<&str>) -> windows::core::Result<Option<String>> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(
            dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
        )?;
        dialog.SetTitle(&HSTRING::from("Where should the apps go?"))?;
        if let Some(start) = start {
            // A folder that has gone simply leaves the dialog's own default.
            if let Ok(item) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(start), None)
            {
                let _ = dialog.SetFolder(&item);
            }
        }
        if let Err(error) = dialog.Show(Some(owner)) {
            return if error.code() == ERROR_CANCELLED.to_hresult() {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = name.to_string();
        CoTaskMemFree(Some(name.0 as *const _));
        Ok(Some(path.map_err(|_| windows::core::Error::empty())?))
    }
}

#[tauri::command]
fn show_in_folder(path: String) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // A Windows path cannot contain a quote, so this cannot break out of it.
    if path.contains('"') {
        return Err("not a file path".into());
    }
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn()
        .map(drop)
        .map_err(|error| error.to_string())
}

fn fail(message: &str) -> ! {
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from("WebWrap Generator"),
            MB_OK | MB_ICONERROR,
        );
    }
    std::process::exit(1)
}

fn main() {
    let data = data_dir().unwrap_or_else(|error| fail(&error));
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            discover,
            pick_folder,
            generate,
            show_in_folder
        ])
        .setup(move |app| {
            let mut builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("WebWrap Generator")
                    .inner_size(760.0, 860.0)
                    .data_directory(data.clone())
                    .use_https_scheme(true)
                    // Everything a fetched site supplies is shown in this page,
                    // which can write executables: it never leaves its origin.
                    .on_navigation(|url| url.origin().ascii_serialization() == ORIGIN)
                    .on_new_window(|_, _| NewWindowResponse::Deny);
            // For tests only, as in the runtime: a DevTools port.
            if let Ok(extra) = std::env::var("WEBWRAP_TEST_BROWSER_ARGUMENTS") {
                if !extra.trim().is_empty() {
                    builder = builder
                        .additional_browser_args(&format!("{WRY_BROWSER_ARGS} {}", extra.trim()));
                }
            }
            if let Err(error) = builder.build() {
                fail(&format!(
                    "the generator window could not be opened: {error}"
                ));
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(error) = result {
        fail(&format!("the generator could not start: {error}"));
    }
}
