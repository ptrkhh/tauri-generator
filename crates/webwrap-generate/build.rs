//! Embeds the runtime templates named by WEBWRAP_TEMPLATE_X64 and
//! WEBWRAP_TEMPLATE_ARM64. Unset, the files are empty and the generator
//! needs --template-x64 and --template-arm64 at run time.

use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for (var, name) in [
        ("WEBWRAP_TEMPLATE_X64", "template-x64.exe"),
        ("WEBWRAP_TEMPLATE_ARM64", "template-arm64.exe"),
    ] {
        println!("cargo:rerun-if-env-changed={var}");
        let bytes = match env::var_os(var) {
            Some(path) => {
                println!("cargo:rerun-if-changed={}", PathBuf::from(&path).display());
                fs::read(&path).unwrap_or_else(|error| panic!("{var}={path:?}: {error}"))
            }
            None => Vec::new(),
        };
        fs::write(out.join(name), bytes).unwrap();
    }
}
