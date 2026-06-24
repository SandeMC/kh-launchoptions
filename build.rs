fn main() {
    // On Windows, set the subsystem to "windows" so no console window appears.
    #[cfg(target_os = "windows")]
    println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
    #[cfg(target_os = "windows")]
    println!("cargo:rustc-link-arg=/ENTRY:mainCRTStartup");
}