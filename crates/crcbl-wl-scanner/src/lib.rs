//! Crucible's Wayland protocol code generator.
//!
//! Protocol XML in, Rust marshalling code out. This is the "our own build-time
//! codegen" half of `docs/plan/15-windowing.md`'s Linux policy: libwayland-client
//! owns the connection and the proxy objects because the Vulkan WSI ABI forces
//! it, and **everything above `wl_proxy_marshal_array_flags` is ours** — no
//! `wayland-client`, no `wayland-scanner`, no `smithay-client-toolkit`.
//!
//! # Why a crate rather than a module inside `build.rs`
//!
//! Because of the last paragraph of this page. `wl_message.signature` strings
//! and `wl_message.types` tables are the classic source of silent Wayland
//! corruption: a wrong character or a shifted index compiles cleanly, raises no
//! protocol error, and mis-encodes the wire. That makes them exactly the code
//! that has to be unit-tested — and a module living inside `build.rs` is
//! invisible to `cargo nextest`. As a workspace member this crate's tests run
//! in the normal suite, including the golden comparison against
//! `wayland-scanner`'s own output for real interfaces.
//!
//! The cost is one build-dependency edge. It has no dependencies of its own, on
//! purpose: anything it pulled in would be compiled for the host on every clean
//! engine build.
//!
//! # Adding a protocol
//!
//! One line in `crates/crcbl-shell/build.rs`, next to the vendored XML:
//!
//! ```text
//! ("xdg_decoration", "wayland-protocols/xdg-decoration-unstable-v1.xml"),
//! ```
//!
//! Cross-protocol interface references (`xdg-shell` naming `wl_surface`)
//! resolve across the whole set, so the new file may refer to interfaces in any
//! other. Nothing in this crate is edited for `wl_seat` (P0.5b) or
//! `data-device` (P0.5c) — they are already generated, since both live in
//! `wayland.xml`.
//!
//! # Example
//!
//! ```
//! use crcbl_wl_scanner::{Options, Source, emit, parse_protocol};
//!
//! let protocol = parse_protocol(
//!     r#"<protocol name="demo">
//!          <interface name="demo_thing" version="2">
//!            <request name="set_size">
//!              <arg name="width" type="int"/>
//!              <arg name="height" type="int"/>
//!            </request>
//!            <event name="done" since="2"/>
//!          </interface>
//!        </protocol>"#,
//! )?;
//! assert_eq!(protocol.interfaces[0].requests[0].signature(), "ii");
//! assert_eq!(protocol.interfaces[0].events[0].signature(), "2");
//!
//! let generated = emit(
//!     &[Source { module: "demo", protocol }],
//!     &Options { ffi_path: "crate::ffi" },
//! )?;
//! assert!(generated.contains("pub unsafe fn set_size("));
//! assert!(generated.contains("pub enum Event {"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod emit;
pub mod model;
pub mod xml;

pub use emit::{EmitError, Options, Source, emit};
pub use model::{
    Arg, ArgType, Enumeration, Interface, Message, Protocol, ScanError, parse_protocol,
};
pub use xml::XmlError;

/// Parses and generates in one call, for a caller that has the XML text and
/// wants the Rust.
///
/// # Errors
///
/// The parse error of the first malformed file, or [`EmitError`] if an
/// interface reference cannot be resolved across the whole set.
pub fn generate(
    files: &[(&str, &str)],
    options: &Options<'_>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut sources = Vec::with_capacity(files.len());
    for (module, xml) in files {
        sources.push(Source {
            module,
            protocol: parse_protocol(xml)?,
        });
    }
    Ok(emit(&sources, options)?)
}
