//! Generates the core protocol and wlr-output-management from XML, against
//! `crcbl_wayland::ffi` — the build step a program outside the engine runs.

use std::path::{Path, PathBuf};

use crcbl_wl_scanner::{Options, Source, parse_protocol};

/// `wayland.xml` is the shell's vendored copy: the scanner resolves the
/// `wl_output` that wlr-output-management names only within one emit set, so
/// an outside program generates the core protocol beside its own.
const PROTOCOLS: &[(&str, &str)] = &[
    ("wayland", "../crcbl-shell/wayland-protocols/wayland.xml"),
    (
        "wlr_output_management",
        "protocols/wlr-output-management-unstable-v1.xml",
    ),
];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(desktop_unix)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(os.as_str(), "linux" | "freebsd") {
        println!("cargo::rustc-cfg=desktop_unix");
    }

    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets this"));
    let mut sources = Vec::with_capacity(PROTOCOLS.len());
    for (module, relative) in PROTOCOLS {
        let path = manifest.join(relative);
        println!("cargo::rerun-if-changed={}", path.display());
        let xml = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let protocol = parse_protocol(&xml)
            .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
        sources.push(Source { module, protocol });
    }

    let generated = crcbl_wl_scanner::emit(
        &sources,
        &Options {
            ffi_path: "crcbl_wayland::ffi",
        },
    )
    .unwrap_or_else(|error| panic!("cannot generate Wayland protocol code: {error}"));

    let out = Path::new(&std::env::var_os("OUT_DIR").expect("cargo sets this")).join("protocol.rs");
    std::fs::write(&out, generated)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", out.display()));
}
