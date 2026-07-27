//! Generates the Wayland protocol marshalling code.
//!
//! `docs/plan/15-windowing.md`: "we generate marshaling from the protocol XMLs
//! with our own build-time codegen, emitting `wl_proxy_marshal_flags` calls
//! against libwayland-client's connection". This is that build step;
//! `crcbl-wl-scanner` is the generator, and it is a workspace crate rather than
//! a module in here so that its output can be unit-tested against
//! `wayland-scanner`'s.
//!
//! # Adding a protocol
//!
//! One line in [`PROTOCOLS`], plus the XML in `wayland-protocols/`. Nothing
//! else — not this file's logic, not the generator. That is the property P0.5b
//! (`xdg-decoration`, `pointer-constraints`, `relative-pointer`) and P0.5c
//! (`data-device`, which is already in `wayland.xml`) are shaped around.
//!
//! # Why it runs on every target
//!
//! The generated file is pure text and costs a few milliseconds, so it is
//! produced unconditionally rather than under `CARGO_CFG_TARGET_OS`. The
//! `include!` that consumes it lives behind `#[cfg(target_os = "linux")]`, so
//! on Windows and macOS the output is simply never compiled — and a mistake in
//! the generator still fails the build on those runners, where it would
//! otherwise hide until someone tried a Linux build.

use std::path::{Path, PathBuf};

use crcbl_wl_scanner::{Options, Source, parse_protocol};

/// The protocols this build generates, as `(Rust module name, XML path)`.
///
/// Order fixes the order of the emitted modules and nothing else: interface
/// references resolve across the whole set, so `xdg-shell` may name
/// `wl_surface` regardless of which is listed first.
const PROTOCOLS: &[(&str, &str)] = &[
    ("wayland", "wayland-protocols/wayland.xml"),
    ("xdg_shell", "wayland-protocols/xdg-shell.xml"),
];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

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
            ffi_path: "crate::wayland::ffi",
        },
    )
    .unwrap_or_else(|error| panic!("cannot generate Wayland protocol code: {error}"));

    let out = Path::new(&std::env::var_os("OUT_DIR").expect("cargo sets this"))
        .join("wayland_protocol.rs");
    std::fs::write(&out, generated)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", out.display()));
}
