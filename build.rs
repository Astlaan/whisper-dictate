use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rustc-link-search=native=libs"); // Tells Rust to look in `libs/`
    println!("cargo:rustc-link-lib=static=mp3lame"); // Link against 'mp3lame.lib'

    // Determine the target directory (debug or release)
    let profile = env::var("PROFILE").unwrap();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let target_dir = Path::new(&manifest_dir).join("target").join(&profile);

    // Define source and destination paths for the DLL
    let src_dll_path = Path::new(&manifest_dir).join("libs").join("libmp3lame.dll");
    let dest_dll_path = target_dir.join("libmp3lame.dll");

    // Copy the DLL
    fs::copy(&src_dll_path, &dest_dll_path)
        .expect(&format!("Failed to copy {} to {}", src_dll_path.display(), dest_dll_path.display()));
}
