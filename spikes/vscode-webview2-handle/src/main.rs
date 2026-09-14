#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("the vscode WebView2 spike must be built and run on Windows");

use std::{ffi::OsString, fs, path::PathBuf};
use tauri::{WebviewUrl, WebviewWindowBuilder};

mod webview2_bridge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Probe,
    Vscode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchRequest {
    mode: Mode,
    path: PathBuf,
}

fn parse_request(args: impl IntoIterator<Item = OsString>) -> Result<LaunchRequest, String> {
    let args: Vec<_> = args.into_iter().collect();
    let (mode, path) = match args.as_slice() {
        [path] => (Mode::Vscode, path),
        [flag, path] if flag == "--probe" => (Mode::Probe, path),
        _ => return Err("usage: vscode-webview2-handle-spike [--probe] <file>".into()),
    };
    let path = fs::canonicalize(path)
        .map_err(|_| "the selected path is not an existing file".to_string())?;
    if !path.is_file() {
        return Err("the selected path is not an existing file".into());
    }
    Ok(LaunchRequest { mode, path })
}

fn run(request: LaunchRequest) {
    tauri::Builder::default()
        .setup(move |app| {
            let url = match request.mode {
                Mode::Probe => WebviewUrl::App("probe.html".into()),
                Mode::Vscode => WebviewUrl::External("https://vscode.dev/".parse().unwrap()),
            };
            let window = WebviewWindowBuilder::new(app, "main", url)
                .title("VS Code Handle Spike")
                .initialization_script(include_str!("vscode.js"))
                .build()?;
            webview2_bridge::install(&window, request.clone())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run vscode WebView2 spike");
}

fn main() {
    let request = parse_request(std::env::args_os().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    run(request);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_file() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "webwrap-vscode-spike-{}-{:?}.md",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, b"before\n").unwrap();
        path
    }

    #[test]
    fn one_path_selects_vscode_mode_and_canonicalizes() {
        let path = test_file();
        let request = parse_request([path.clone().into_os_string()]).unwrap();
        assert_eq!(request.mode, Mode::Vscode);
        assert_eq!(request.path, std::fs::canonicalize(&path).unwrap());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn probe_flag_selects_probe_mode() {
        let path = test_file();
        let request = parse_request(["--probe".into(), path.clone().into_os_string()]).unwrap();
        assert_eq!(request.mode, Mode::Probe);
        assert_eq!(request.path, std::fs::canonicalize(&path).unwrap());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_file_is_rejected() {
        let path = std::env::temp_dir().join("webwrap-vscode-spike-missing.md");
        let result = parse_request([path.into_os_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_argument_shapes_are_rejected() {
        for args in [
            Vec::<std::ffi::OsString>::new(),
            vec!["one".into(), "two".into()],
            vec!["--unknown".into(), "file".into()],
        ] {
            assert!(parse_request(args).is_err());
        }
    }
}
