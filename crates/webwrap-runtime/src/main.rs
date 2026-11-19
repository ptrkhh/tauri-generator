#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("webwrap-runtime must be built and run on Windows");

mod embedded_spec;
mod fatal;
mod inspect;

use std::path::PathBuf;

use tauri::webview::NewWindowResponse;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use webwrap_spec::{nav_verdict, AppSpec, NavEvent, Verdict};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR,
    SM_CXICON, SM_CXSMICON, SW_SHOWNORMAL, WM_SETICON,
};

enum Mode {
    Run,
    InspectSelf { check_urls: Vec<String> },
}

/// Parse arguments.
///
/// A positional file path is rejected for now. Sub-project D adds the file
/// bridge and relaxes this; accepting a path before anything can open it
/// would look like success and do nothing.
fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Mode, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Mode::Run);
    };
    if first != "--inspect-self" {
        return Err(format!("unknown argument: {first}"));
    }
    let mut check_urls = Vec::new();
    while let Some(flag) = args.next() {
        if flag != "--check-url" {
            return Err(format!("unknown argument: {flag}"));
        }
        let value = args
            .next()
            .ok_or_else(|| "missing value for --check-url".to_string())?;
        if value.starts_with("--") {
            return Err("missing value for --check-url".into());
        }
        check_urls.push(value);
    }
    Ok(Mode::InspectSelf { check_urls })
}

/// Storage location for this app identity.
///
/// Every generated executable shares one compiled runtime, so on Windows
/// isolation has to be explicit; on Android a unique application ID would
/// provide it for free.
fn data_dir(spec: &AppSpec) -> Result<PathBuf, String> {
    let local = std::env::var("LOCALAPPDATA").map_err(|_| {
        "LOCALAPPDATA is not set, so this app cannot create its own isolated storage".to_string()
    })?;
    if local.is_empty() {
        return Err(
            "LOCALAPPDATA is empty, so this app cannot create its own isolated storage".into(),
        );
    }
    Ok(PathBuf::from(local).join("WebWrap").join(&spec.app_id))
}

/// Hand a URL to the system browser. Only reached for verdict
/// `OpenExternally`, which `nav_verdict` returns for http and https alone.
fn open_externally(url: &str) {
    let url = HSTRING::from(url);
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &url,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Show this executable's own icon on the window and taskbar. Tauri applies
/// the placeholder compiled into the template; the generator replaces the PE
/// icon group, which editpe names `MAINICON`, so only that resource is per app.
/// If it is missing the placeholder stays, which the GUI acceptance check sees.
fn use_main_icon(hwnd: HWND) {
    unsafe {
        let Ok(module) = GetModuleHandleW(PCWSTR::null()) else {
            return;
        };
        for (kind, metric) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)] {
            let size = GetSystemMetrics(metric);
            // Never destroyed: the window shows it until the process exits.
            if let Ok(icon) = LoadImageW(
                Some(module.into()),
                w!("MAINICON"),
                IMAGE_ICON,
                size,
                size,
                LR_DEFAULTCOLOR,
            ) {
                SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(kind as usize)),
                    Some(LPARAM(icon.0 as isize)),
                );
            }
        }
    }
}

fn run(spec: AppSpec, data_dir: PathBuf) -> ! {
    // Validated as an https URL by AppSpec::validate, so parsing cannot fail.
    let url: tauri::Url = spec
        .launch_url
        .parse()
        .expect("launchUrl was validated as an https URL");
    let title = spec.display_name;

    let result = tauri::Builder::default()
        .setup(move |app| {
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title(title)
                // Every generated app shares this compiled runtime and its
                // tauri.conf.json identifier, so without this they would all
                // share Tauri's default WebView2 folder.
                .data_directory(data_dir)
                .on_navigation(|url| nav_verdict(url.as_str(), NavEvent::TopLevel).is_allow())
                .on_new_window(|url, _features| {
                    if nav_verdict(url.as_str(), NavEvent::NewWindow) == Verdict::OpenExternally {
                        open_externally(url.as_str());
                    }
                    NewWindowResponse::Deny
                })
                .build()?;
            use_main_icon(window.hwnd()?);
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(error) = result {
        // A release build has no console, so a panic message would vanish.
        fatal::abort(
            &format!("the app window could not be started: {error}"),
            true,
        );
    }
    std::process::exit(0)
}

fn main() {
    let mode = match parse_args(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let interactive = matches!(mode, Mode::Run);

    let bytes = match embedded_spec::read() {
        Ok(bytes) => bytes,
        Err(error) => fatal::abort(&error, interactive),
    };
    let spec = match AppSpec::from_slice(&bytes) {
        Ok(spec) => spec,
        Err(error) => fatal::abort(&error.to_string(), interactive),
    };
    let data_dir = match data_dir(&spec) {
        Ok(path) => path,
        Err(error) => fatal::abort(&error, interactive),
    };

    match mode {
        Mode::InspectSelf { check_urls } => inspect::run(&spec, &data_dir, &check_urls),
        Mode::Run => run(spec, data_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_means_run() {
        assert!(matches!(
            parse_args(Vec::<String>::new()).unwrap(),
            Mode::Run
        ));
    }

    #[test]
    fn inspect_self_collects_repeated_check_urls() {
        let mode = parse_args(
            [
                "--inspect-self",
                "--check-url",
                "https://a.test/",
                "--check-url",
                "http://b.test/",
            ]
            .map(String::from),
        )
        .unwrap();
        match mode {
            Mode::InspectSelf { check_urls } => {
                assert_eq!(check_urls, ["https://a.test/", "http://b.test/"]);
            }
            Mode::Run => panic!("expected inspect mode"),
        }
    }

    #[test]
    fn malformed_argument_shapes_are_rejected() {
        for args in [
            vec!["--nonsense"],
            vec!["file.psd"],
            vec!["--inspect-self", "--check-url"],
            vec!["--inspect-self", "--check-url", "--nonsense"],
            vec!["--inspect-self", "extra"],
        ] {
            assert!(
                parse_args(args.iter().map(|a| a.to_string())).is_err(),
                "{args:?}"
            );
        }
    }
}
