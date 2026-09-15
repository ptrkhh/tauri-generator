#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("the Photopea WebView2 spike must be built and run on Windows");

use std::{ffi::OsString, fs, path::PathBuf};

use tauri::{WebviewUrl, WebviewWindowBuilder};

mod webview2_bridge;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchRequest {
    path: PathBuf,
}

fn parse_request(args: impl IntoIterator<Item = OsString>) -> Result<LaunchRequest, String> {
    let args: Vec<_> = args.into_iter().collect();
    let [path] = args.as_slice() else {
        return Err("usage: photopea-windows-writeback-spike <file>".into());
    };
    let path = fs::canonicalize(path)
        .map_err(|_| "the selected path is not an existing file".to_string())?;
    if !path.is_file() {
        return Err("the selected path is not an existing file".into());
    }
    Ok(LaunchRequest { path })
}

fn run(request: LaunchRequest) {
    tauri::Builder::default()
        .setup(move |app| {
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("photopea.html".into()))
                    .title("Photopea Writeback Spike")
                    .build()?;
            webview2_bridge::install(&window, request.clone())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Photopea WebView2 spike");
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
    use std::{
        ffi::OsString,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::parse_request;

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "webwrap-photopea-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn one_existing_path_selects_photopea_and_canonicalizes() {
        let path = temp_path("request");
        fs::write(&path, b"8BPS").unwrap();

        let request = parse_request([path.clone().into_os_string()]).unwrap();

        assert_eq!(request.path, fs::canonicalize(&path).unwrap());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_path_is_rejected() {
        let path = temp_path("missing");

        assert!(parse_request([path.into_os_string()]).is_err());
    }

    #[test]
    fn directory_path_is_rejected() {
        let path = temp_path("directory");
        fs::create_dir(&path).unwrap();

        assert!(parse_request([path.clone().into_os_string()]).is_err());
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn zero_multiple_and_unknown_arguments_are_rejected() {
        let first = temp_path("first");
        let second = temp_path("second");
        fs::write(&first, b"8BPS").unwrap();
        fs::write(&second, b"8BPS").unwrap();

        for args in [
            Vec::<OsString>::new(),
            vec![
                first.clone().into_os_string(),
                second.clone().into_os_string(),
            ],
            vec![OsString::from("--unknown"), first.clone().into_os_string()],
        ] {
            assert!(parse_request(args).is_err());
        }

        fs::remove_file(first).unwrap();
        fs::remove_file(second).unwrap();
    }
}
