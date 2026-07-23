fn main() {
    // `tauri_build::build()` bakes the icons into the binary: `generate_context!` embeds the window
    // icon, and on Windows the resource step embeds `icon.ico` for Explorer/taskbar. Cargo does not
    // know those files are build inputs, so changing the brand mark alone leaves the crate "fresh"
    // — the build finishes in a second and the *old* icons stay compiled in. Symptom: new icons on
    // disk, stale icons on screen, and a rebuild that appears to do nothing.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");

    tauri_build::build()
}
