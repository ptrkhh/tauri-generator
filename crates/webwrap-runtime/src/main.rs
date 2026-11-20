#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("webwrap-runtime must be built and run on Windows");

mod bridge;
mod embedded_spec;
mod hardening;
mod inspect;
mod report;
mod save;
mod single_instance;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc;

use tauri::{WebviewUrl, WebviewWindowBuilder};
use webwrap_bridge::{resolve, MAX_FILES};
use webwrap_spec::{nav_verdict, AppSpec, NavEvent};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR,
    SM_CXICON, SM_CXSMICON, WM_SETICON,
};

use crate::bridge::Bridge;
use crate::single_instance::{Forwarded, Role};

enum Mode {
    Run { files: Vec<OsString> },
    InspectSelf { check_urls: Vec<String> },
}

/// Parse arguments: `--inspect-self [--check-url <url>]...`, or files.
///
/// Every other argument is a file, including one that starts with `--`, so a
/// dropped file never becomes a usage error. Explorer passes absolute paths.
fn parse_args(args: Vec<OsString>) -> Result<Mode, String> {
    if args.first().is_none_or(|first| first != "--inspect-self") {
        return Ok(Mode::Run { files: args });
    }
    let mut rest = args.into_iter().skip(1).map(|arg| {
        arg.into_string()
            .map_err(|_| "arguments must be valid Unicode".to_string())
    });
    let mut check_urls = Vec::new();
    while let Some(flag) = rest.next() {
        let flag = flag?;
        if flag != "--check-url" {
            return Err(format!("unknown argument: {flag}"));
        }
        let value = rest
            .next()
            .ok_or_else(|| "missing value for --check-url".to_string())??;
        if value.starts_with("--") {
            return Err("missing value for --check-url".into());
        }
        check_urls.push(value);
    }
    Ok(Mode::InspectSelf { check_urls })
}

/// Resolve file arguments to absolute paths of existing files, at most
/// `MAX_FILES`. Returns the good paths and one line per bad argument.
fn resolve_files(args: &[OsString]) -> (Vec<PathBuf>, Vec<String>) {
    let mut good = Vec::new();
    let mut bad = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let shown = arg.to_string_lossy();
        if index >= MAX_FILES {
            bad.push(format!(
                "{shown}: too many files at once (the limit is {MAX_FILES})"
            ));
            continue;
        }
        match resolve(arg) {
            Ok(path) => good.push(path),
            Err(reason) => bad.push(format!("{shown}: {reason}")),
        }
    }
    (good, bad)
}

fn bad_files_message(bad: &[String]) -> String {
    format!("These files could not be opened:\n\n{}", bad.join("\n"))
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

fn run(
    spec: AppSpec,
    data_dir: PathBuf,
    files: Vec<PathBuf>,
    pipe: Option<std::os::windows::io::OwnedHandle>,
) -> ! {
    // Validated as an https URL by AppSpec::validate, so parsing cannot fail.
    let url: tauri::Url = spec
        .launch_url
        .parse()
        .expect("launchUrl was validated as an https URL");
    let title = spec.display_name;
    let trusted_origins = spec.trusted_origins;

    // Started before the window exists so a later launch is answered at once;
    // its files wait in the channel until the bridge is ready.
    let (forwarded, inbox) = mpsc::channel();
    if let Some(pipe) = pipe {
        single_instance::serve(pipe, spec.app_id, forwarded);
    }

    let result = tauri::Builder::default()
        .setup(move |app| {
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title(title)
                // Every generated app shares this compiled runtime and its
                // tauri.conf.json identifier, so without this they would all
                // share Tauri's default WebView2 folder.
                .data_directory(data_dir)
                .on_navigation(|url| nav_verdict(url.as_str(), NavEvent::TopLevel).is_allow())
                // Let a dropped file reach the site, as in a browser. Tauri's
                // own handler would swallow it.
                .disable_drag_drop_handler()
                .build();
            // Tauri turns an Err from setup into a panic, which a release
            // build cannot show, so failures here must abort directly.
            let window = match window {
                Ok(window) => window,
                Err(error) => report::abort(
                    &format!("the app window could not be opened: {error}"),
                    true,
                ),
            };
            // Only the icon depends on this; the app still works without it.
            if let Ok(hwnd) = window.hwnd() {
                use_main_icon(hwnd);
            }
            let bridge = match Bridge::install(&window, trusted_origins, files) {
                Ok(bridge) => bridge,
                Err(error) => report::abort(
                    &format!("this app's safety checks could not be installed: {error}"),
                    true,
                ),
            };
            // Closing waits for a save being written: the process ends with
            // the window, which would kill the write half done.
            let holder = bridge.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if holder.hold_close() {
                        api.prevent_close();
                    }
                }
            });
            std::thread::spawn(move || {
                for files in inbox {
                    bridge.open_forwarded(files);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(error) = result {
        // A release build has no console, so a panic message would vanish.
        report::abort(
            &format!("the app window could not be started: {error}"),
            true,
        );
    }
    std::process::exit(0)
}

fn main() {
    let mode = match parse_args(std::env::args_os().skip(1).collect()) {
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let interactive = matches!(mode, Mode::Run { .. });

    let bytes = match embedded_spec::read() {
        Ok(bytes) => bytes,
        Err(error) => report::abort(&error, interactive),
    };
    let spec = match AppSpec::from_slice(&bytes) {
        Ok(spec) => spec,
        Err(error) => report::abort(&error.to_string(), interactive),
    };
    let data_dir = match data_dir(&spec) {
        Ok(path) => path,
        Err(error) => report::abort(&error, interactive),
    };

    let files = match mode {
        Mode::InspectSelf { check_urls } => inspect::run(&spec, &data_dir, &check_urls),
        Mode::Run { files } => files,
    };
    let (files, bad) = resolve_files(&files);
    match single_instance::claim(&spec.app_id) {
        Ok(Role::Secondary) => {
            let forwarded = single_instance::forward(&spec.app_id, &files);
            if let Ok(Forwarded::NotOurs) = forwarded {
                // Someone else created the pipe first. Nothing was sent to
                // it; this launch runs on its own instead.
                report::log("single-instance-foreign-pipe");
                if !bad.is_empty() {
                    report::error_box_async(bad_files_message(&bad));
                }
                run(spec, data_dir, files, None)
            }
            let mut message = if bad.is_empty() {
                String::new()
            } else {
                bad_files_message(&bad) + "\n\n"
            };
            if let Err(error) = &forwarded {
                message +=
                    &format!("This app is already running, but {error}. Close it and try again.");
            }
            if !message.is_empty() {
                report::error_box(message.trim_end());
            }
            std::process::exit(if forwarded.is_ok() { 0 } else { 1 });
        }
        Ok(Role::Primary(pipe)) => {
            if !bad.is_empty() {
                report::error_box_async(bad_files_message(&bad));
            }
            run(spec, data_dir, files, Some(pipe))
        }
        Err(error) => {
            // Still usable; only a later launch cannot reach this window.
            report::log(format_args!("single-instance-unavailable {error}"));
            if !bad.is_empty() {
                report::error_box_async(bad_files_message(&bad));
            }
            run(spec, data_dir, files, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_means_run_with_no_files() {
        assert!(matches!(
            parse_args(Vec::new()).unwrap(),
            Mode::Run { files } if files.is_empty()
        ));
    }

    #[test]
    fn anything_but_inspect_self_is_a_list_of_files() {
        let mode = parse_args(args(&["file.psd", "--nonsense", "C:\\x\\b.md"])).unwrap();
        assert!(matches!(mode, Mode::Run { files } if files.len() == 3));
    }

    #[test]
    fn inspect_self_collects_repeated_check_urls() {
        let mode = parse_args(args(&[
            "--inspect-self",
            "--check-url",
            "https://a.test/",
            "--check-url",
            "http://b.test/",
        ]))
        .unwrap();
        match mode {
            Mode::InspectSelf { check_urls } => {
                assert_eq!(check_urls, ["https://a.test/", "http://b.test/"]);
            }
            Mode::Run { .. } => panic!("expected inspect mode"),
        }
    }

    #[test]
    fn malformed_inspect_arguments_are_rejected() {
        for values in [
            vec!["--inspect-self", "--check-url"],
            vec!["--inspect-self", "--check-url", "--nonsense"],
            vec!["--inspect-self", "extra"],
        ] {
            assert!(parse_args(args(&values)).is_err(), "{values:?}");
        }
    }

    #[test]
    fn files_resolve_with_a_reason_for_each_bad_one_and_a_limit() {
        let dir = std::env::temp_dir().join(format!("webwrap-args-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.psd");
        std::fs::write(&file, b"8BPS").unwrap();
        let mut values = vec![file.clone().into_os_string(), dir.join("gone.psd").into()];
        let (good, bad) = resolve_files(&values);
        assert_eq!(good, std::slice::from_ref(&file));
        assert!(bad[0].contains("does not exist"), "{bad:?}");
        values = vec![file.clone().into_os_string(); MAX_FILES + 2];
        let (good, bad) = resolve_files(&values);
        assert_eq!((good.len(), bad.len()), (MAX_FILES, 2));
        assert!(bad[0].contains("too many files"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
