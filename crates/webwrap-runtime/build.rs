use std::{env, fs, path::PathBuf};

fn main() {
    let icon = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("icon.ico");
    // 1x1 transparent 32-bit DIB wrapped in the minimal ICO container. This is
    // a placeholder: the generator replaces the main icon during
    // specialization. It exists so that replacement has something to replace.
    fs::write(
        &icon,
        [
            0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0, 48, 0, 0, 0, 22, 0, 0, 0, 40, 0, 0, 0, 1, 0,
            0, 0, 2, 0, 0, 0, 1, 0, 32, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    )
    .unwrap();
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new().window_icon_path(icon)),
    )
    .unwrap();
}
