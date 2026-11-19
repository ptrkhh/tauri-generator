#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("webwrap-runtime must be built and run on Windows");

fn main() {
    // Replaced in Task 6. Exists so the template can be built and specialized.
    std::process::exit(0);
}
