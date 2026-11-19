fn main() {
    // icons/icon.ico is a 1x1 transparent placeholder. tauri-build embeds it as
    // the main icon, so the generator's icon replacement has something to
    // replace, and generate_context! compiles it in as Tauri's default window
    // icon, which the runtime overrides with the replaced resource.
    tauri_build::build()
}
