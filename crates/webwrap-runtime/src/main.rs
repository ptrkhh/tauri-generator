#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("webwrap-runtime must be built and run on Windows");

mod embedded_spec;
mod fatal;
mod inspect;

use std::path::PathBuf;

use webwrap_spec::AppSpec;

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
        Mode::Run => {
            // Task 8 opens the window here.
            std::process::exit(0)
        }
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
