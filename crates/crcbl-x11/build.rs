//! Sets `desktop_unix` on the targets libxcb exists on.
//!
//! The same cfg, for the same targets, as `crcbl-shell`'s build script: the
//! binding is compiled only where the shell's X11 backend is.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(desktop_unix)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(os.as_str(), "linux" | "freebsd") {
        println!("cargo::rustc-cfg=desktop_unix");
    }
}
