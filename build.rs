fn main() {
    println!("cargo:rustc-link-search=native=libs"); // Tells Rust to look in `libs/`
    println!("cargo:rustc-link-lib=static=mp3lame"); // Link against 'mp3lame.lib'
}
