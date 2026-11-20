//! The generator's command line, until sub-project F wraps the same library
//! in a window.

use std::fs;
use std::io::Read;
use std::path::PathBuf;

use serde_json::{json, Value};
use webwrap_pack::spec::MAX_ICON_BYTES;
use webwrap_pack::{generate, inspect, AppInput, PackError, Templates};
use webwrap_spec::{Adapter, Assoc};

const EMBEDDED_X64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/template-x64.exe"));
const EMBEDDED_ARM64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/template-arm64.exe"));

const USAGE: &str = "usage:
  webwrap-generate generate --url <website> --name <display name> --icon <png> --out <folder>
                            [--slug <slug>] [--adapter none|photopea|vscodeDev]
                            [--assoc <.ext> <mime type> <description>]...
                            [--template-x64 <exe> --template-arm64 <exe>]
  webwrap-generate inspect --input <exe>";

#[derive(Debug)]
struct GenerateArgs {
    input: AppInput,
    icon: PathBuf,
    out: PathBuf,
    templates: Option<(PathBuf, PathBuf)>,
}

#[derive(Debug)]
enum Command {
    Generate(GenerateArgs),
    Inspect(PathBuf),
}

fn parse_args(args: Vec<String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let command = args.next().ok_or("missing subcommand")?;
    let mut flags: Vec<(String, Vec<String>)> = Vec::new();
    while let Some(flag) = args.next() {
        let count = match flag.as_str() {
            "--assoc" => 3,
            "--url" | "--name" | "--icon" | "--out" | "--slug" | "--adapter" | "--template-x64"
            | "--template-arm64" | "--input" => 1,
            _ => return Err(format!("unknown argument: {flag}")),
        };
        if flag != "--assoc" && flags.iter().any(|(seen, _)| *seen == flag) {
            return Err(format!("{flag} given twice"));
        }
        let mut values = Vec::new();
        for _ in 0..count {
            match args.next() {
                Some(value) if !value.starts_with("--") => values.push(value),
                _ => return Err(format!("missing value for {flag}")),
            }
        }
        flags.push((flag, values));
    }
    let take = |name: &str| -> Option<String> {
        flags
            .iter()
            .find(|(flag, _)| flag == name)
            .map(|(_, values)| values[0].clone())
    };
    let require = |name: &str| take(name).ok_or_else(|| format!("missing {name}"));
    let allowed: &[&str] = match command.as_str() {
        "generate" => &[
            "--url",
            "--name",
            "--icon",
            "--out",
            "--slug",
            "--adapter",
            "--assoc",
            "--template-x64",
            "--template-arm64",
        ],
        "inspect" => &["--input"],
        other => return Err(format!("unknown subcommand: {other}")),
    };
    if let Some((flag, _)) = flags
        .iter()
        .find(|(flag, _)| !allowed.contains(&flag.as_str()))
    {
        return Err(format!("{flag} does not apply to {command}"));
    }
    if command == "inspect" {
        return Ok(Command::Inspect(require("--input")?.into()));
    }
    let adapter = match take("--adapter") {
        None => Adapter::None,
        Some(name) => serde_json::from_value(Value::String(name.clone()))
            .map_err(|_| format!("unknown adapter: {name}"))?,
    };
    let templates = match (take("--template-x64"), take("--template-arm64")) {
        (Some(x64), Some(arm64)) => Some((x64.into(), arm64.into())),
        (None, None) => None,
        _ => return Err("--template-x64 and --template-arm64 must be given together".into()),
    };
    Ok(Command::Generate(GenerateArgs {
        input: AppInput {
            url: require("--url")?,
            display_name: require("--name")?,
            slug: take("--slug"),
            adapter,
            file_associations: flags
                .iter()
                .filter(|(flag, _)| flag == "--assoc")
                .map(|(_, values)| Assoc {
                    extension: values[0].clone(),
                    mime_type: values[1].clone(),
                    description: values[2].clone(),
                })
                .collect(),
        },
        icon: require("--icon")?.into(),
        out: require("--out")?.into(),
        templates,
    }))
}

fn exit(code: i32, message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(code)
}

fn read(path: &PathBuf) -> Vec<u8> {
    fs::read(path)
        .unwrap_or_else(|error| exit(1, format!("could not read {}: {error}", path.display())))
}

fn run_generate(args: GenerateArgs) -> Value {
    let (x64, arm64) = match &args.templates {
        Some((x64, arm64)) => (read(x64), read(arm64)),
        None if EMBEDDED_X64.is_empty() || EMBEDDED_ARM64.is_empty() => exit(
            2,
            "this generator was built without runtime templates; \
             pass --template-x64 and --template-arm64",
        ),
        None => (EMBEDDED_X64.to_vec(), EMBEDDED_ARM64.to_vec()),
    };
    // Read one byte past the limit, so check_icon rejects an oversized file
    // without the whole of it (or an endless device) landing in memory.
    let mut icon = Vec::new();
    fs::File::open(&args.icon)
        .and_then(|file| file.take(MAX_ICON_BYTES as u64 + 1).read_to_end(&mut icon))
        .unwrap_or_else(|error| {
            exit(
                1,
                format!("could not read {}: {error}", args.icon.display()),
            )
        });
    let templates = Templates {
        x64: &x64,
        arm64: &arm64,
    };
    match generate(&args.input, &icon, &args.out, templates) {
        Ok(generated) => json!({
            "appId": generated.app_id,
            "slug": generated.slug,
            "outputs": generated.outputs.iter().map(|output| json!({
                "arch": output.arch.suffix(),
                "path": output.path,
                "replaced": output.replaced.as_str(),
            })).collect::<Vec<_>>(),
        }),
        Err(error @ PackError::Input { .. }) => exit(2, error),
        Err(error) => exit(1, error),
    }
}

fn run_inspect(path: PathBuf) -> Value {
    let report = inspect(&read(&path)).unwrap_or_else(|error| exit(1, error));
    let app_id = report
        .spec
        .as_deref()
        .and_then(|spec| serde_json::from_slice::<Value>(spec).ok())
        .and_then(|spec| spec.get("appId").cloned());
    json!({
        "arch": report.arch.map(|arch| arch.suffix()),
        "appId": app_id,
        "specBytes": report.spec.as_ref().map(Vec::len),
        "iconPresent": report.icon_present,
        "fileDescription": report.file_description,
        "productVersion": report.product_version,
    })
}

fn main() {
    // args() would panic on a non-Unicode argument; that is a usage error.
    let args: Result<Vec<String>, _> = std::env::args_os()
        .skip(1)
        .map(|a| a.into_string())
        .collect();
    let command = args
        .map_err(|_| "arguments must be valid Unicode".to_string())
        .and_then(parse_args)
        .unwrap_or_else(|error| exit(2, format!("{error}\n\n{USAGE}")));
    let report = match command {
        Command::Generate(args) => run_generate(args),
        Command::Inspect(path) => run_inspect(path),
    };
    println!("{report}");
}
