#[cfg(not(windows))]
compile_error!("the Windows PE specialization spike must be built and run on Windows");

pub mod pe;

use std::path::PathBuf;

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
        if value.starts_with('-') {
            return Err(format!("missing value for {flag}"));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &[u8] = br#"{"schemaVersion":1,"appId":"a123","displayName":"Photopea","slug":"photopea","launchUrl":"https://www.photopea.com/"}"#;

    #[test]
    fn parses_commands_and_rejects_flag_values() {
        assert!(matches!(
            parse_args(["inspect", "--input", "a.exe"]),
            Ok(Command::Inspect(_))
        ));
        assert!(matches!(
            parse_args([
                "specialize",
                "--input",
                "a.exe",
                "--output",
                "b.exe",
                "--spec",
                "s",
                "--icon",
                "i",
                "--display-name",
                "A",
                "--version",
                "1"
            ]),
            Ok(Command::Specialize(_))
        ));
        assert!(parse_args(["inspect", "--input", "--output"]).is_err());
        assert!(parse_args([
            "specialize",
            "--input",
            "a.exe",
            "--output",
            "b.exe",
            "--spec",
            "s",
            "--icon",
            "i",
            "--display-name",
            "A",
            "--version",
            "--bad"
        ])
        .is_err());
        assert!(parse_args(Vec::<String>::new()).is_err());
        assert!(parse_args(["unknown", "--input", "a.exe"]).is_err());
    }

    #[test]
    fn validates_spec_shape_and_utf8() {
        assert_eq!(pe::validate_spec(SPEC).unwrap(), SPEC);
        assert!(pe::validate_spec(b"\xff").is_err());
        assert!(pe::validate_spec(br#"[1,2,3]"#).is_err());
        assert!(pe::validate_spec(br#"{}"#).is_err());
        assert!(pe::validate_spec(
            br#"{"schemaVersion":"1","appId":"a","displayName":"d","slug":"s","launchUrl":"u"}"#
        )
        .is_err());
        assert!(pe::validate_spec(
            br#"{"schemaVersion":1,"appId":1,"displayName":"d","slug":"s","launchUrl":"u"}"#
        )
        .is_err());
    }

    #[test]
    fn validates_text_boundaries() {
        assert!(pe::validate_text("a", "field").is_ok());
        assert!(pe::validate_text(&"a".repeat(255), "field").is_ok());
        assert!(pe::validate_text(&"a".repeat(256), "field").is_err());
        assert!(pe::validate_text("a\0", "field").is_err());
        assert!(pe::validate_text("", "field").is_err());
    }
}
