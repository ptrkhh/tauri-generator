use std::env;
use std::fs;

use serde_json::json;
use windows_pe_specialization_spike::{pe, Command};

fn main() {
    let command = match windows_pe_specialization_spike::parse_args(env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let result = match command {
        Command::Specialize(args) => {
            let spec = match fs::read(&args.spec) {
                Ok(spec) => spec,
                Err(error) => fail(1, error.to_string()),
            };
            if let Err(error) = pe::validate_spec(&spec) {
                fail(2, error.to_string());
            }
            match pe::specialize(
                &args.input,
                &args.output,
                &spec,
                &args.icon,
                &args.display_name,
                &args.version,
            ) {
                Ok(metadata) => json!({
                    "output": args.output,
                    "specBytes": metadata.spec.len(),
                    "iconPresent": metadata.icon_present,
                    "fileDescription": metadata.file_description,
                    "productVersion": metadata.product_version,
                }),
                Err(error) => fail(1, error.to_string()),
            }
        }
        Command::Inspect(args) => match pe::read_metadata(&args.input) {
            Ok(metadata) => json!({
                "output": args.input,
                "specBytes": metadata.spec.len(),
                "iconPresent": metadata.icon_present,
                "fileDescription": metadata.file_description,
                "productVersion": metadata.product_version,
            }),
            Err(error) => fail(1, error.to_string()),
        },
    };
    println!("{}", result);
}

fn fail(code: i32, error: String) -> ! {
    eprintln!("{error}");
    std::process::exit(code);
}
