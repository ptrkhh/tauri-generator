use std::env;
use std::path::PathBuf;

use windows_pe_specialization_spike::pe;

#[derive(Debug, PartialEq, Eq)]
pub struct SpecializeArgs {
    pub input: PathBuf,
    pub output: PathBuf,
    pub spec: PathBuf,
    pub icon: PathBuf,
    pub display_name: String,
    pub version: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InspectArgs {
    pub input: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Specialize(SpecializeArgs),
    Inspect(InspectArgs),
}

pub fn parse_args<I, S>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let command = args
        .next()
        .ok_or_else(|| "missing subcommand".to_string())?;
    let mut input = None;
    let mut output = None;
    let mut spec = None;
    let mut icon = None;
    let mut display_name = None;
    let mut version = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--input" => input = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--spec" => spec = Some(PathBuf::from(value)),
            "--icon" => icon = Some(PathBuf::from(value)),
            "--display-name" => display_name = Some(value),
            "--version" => version = Some(value),
            _ => return Err(format!("unknown flag: {flag}")),
        }
    }
    let input = input.ok_or_else(|| "missing --input".to_string())?;
    match command.as_str() {
        "inspect"
            if output.is_none()
                && spec.is_none()
                && icon.is_none()
                && display_name.is_none()
                && version.is_none() =>
        {
            Ok(Command::Inspect(InspectArgs { input }))
        }
        "specialize" => {
            let output = output.ok_or_else(|| "missing --output".to_string())?;
            let spec = spec.ok_or_else(|| "missing --spec".to_string())?;
            let icon = icon.ok_or_else(|| "missing --icon".to_string())?;
            let display_name = display_name.ok_or_else(|| "missing --display-name".to_string())?;
            let version = version.ok_or_else(|| "missing --version".to_string())?;
            if input == output {
                return Err("input and output must differ".into());
            }
            pe::validate_text(&display_name, "display-name").map_err(|error| error.to_string())?;
            pe::validate_text(&version, "version").map_err(|error| error.to_string())?;
            Ok(Command::Specialize(SpecializeArgs {
                input,
                output,
                spec,
                icon,
                display_name,
                version,
            }))
        }
        _ => Err(format!("unknown subcommand: {command}")),
    }
}

fn main() {
    if let Err(error) = parse_args(env::args().skip(1)) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_inputs() {
        assert!(parse_args(Vec::<String>::new()).is_err());
        assert!(parse_args(["unknown", "--input", "a.exe"]).is_err());
        assert!(parse_args(["specialize", "--input", "a.exe"]).is_err());
        assert!(parse_args([
            "specialize",
            "--input",
            "a.exe",
            "--output",
            "a.exe",
            "--spec",
            "s",
            "--icon",
            "i",
            "--display-name",
            "A",
            "--version",
            "1"
        ])
        .is_err());
        assert!(pe::validate_text("", "display-name").is_err());
        assert!(pe::validate_text("a\n", "display-name").is_err());
        assert!(pe::validate_spec(br#"[1, 2, 3]"#).is_err());
    }
}
