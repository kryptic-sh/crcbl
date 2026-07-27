//! The generator, compared against `wayland-scanner`'s own output.
//!
//! Every expected value in this file was read out of
//! `wayland-scanner private-code` run on the same two XML files the shell
//! vendors (`wayland-scanner 1.25.0`). That is the point: a test asserting what
//! *we* think a signature should be proves nothing, because the failure mode is
//! that our idea is wrong. libwayland and the graphics driver read these tables
//! to encode and decode every message, and a wrong character or a shifted index
//! compiles cleanly, raises no protocol error, and mis-encodes the wire.
//!
//! The XML is reached across the workspace on purpose — these tests must pin
//! the files the build actually generates from, not a copy that can drift.

use crcbl_wl_scanner::{Options, Source, emit, parse_protocol};

/// Every XML the shell's `build.rs` feeds the generator outside the
/// `wayland-e2e` feature, in the same order — so this test pins the file the
/// build actually produces rather than a subset of it.
const PROTOCOLS: &[(&str, &str)] = &[
    (
        "wayland",
        include_str!("../../crcbl-shell/wayland-protocols/wayland.xml"),
    ),
    (
        "xdg_shell",
        include_str!("../../crcbl-shell/wayland-protocols/xdg-shell.xml"),
    ),
    (
        "viewporter",
        include_str!("../../crcbl-shell/wayland-protocols/viewporter.xml"),
    ),
    (
        "fractional_scale",
        include_str!("../../crcbl-shell/wayland-protocols/fractional-scale-v1.xml"),
    ),
    (
        "xdg_output",
        include_str!("../../crcbl-shell/wayland-protocols/xdg-output-unstable-v1.xml"),
    ),
    (
        "xdg_decoration",
        include_str!("../../crcbl-shell/wayland-protocols/xdg-decoration-unstable-v1.xml"),
    ),
    (
        "relative_pointer",
        include_str!("../../crcbl-shell/wayland-protocols/relative-pointer-unstable-v1.xml"),
    ),
    (
        "pointer_constraints",
        include_str!("../../crcbl-shell/wayland-protocols/pointer-constraints-unstable-v1.xml"),
    ),
];

fn generated() -> String {
    let sources: Vec<Source<'_>> = PROTOCOLS
        .iter()
        .map(|(module, xml)| Source {
            module,
            protocol: parse_protocol(xml).unwrap_or_else(|error| panic!("{module}.xml: {error}")),
        })
        .collect();
    emit(&sources, &Options::default()).expect("every interface reference resolves")
}

/// The `TYPES` entries of one protocol module, in order.
fn type_table(generated: &str, module: &str) -> Vec<String> {
    let module_start = generated
        .find(&format!("pub mod {module} {{"))
        .expect("the protocol module is emitted");
    let table_start = generated[module_start..]
        .find("static TYPES")
        .expect("the type table is emitted")
        + module_start;
    let body_start = generated[table_start..]
        .find("= [\n")
        .expect("the table opens")
        + table_start
        + 3;
    let body_end = generated[body_start..]
        .find("\n    ];")
        .expect("the table closes")
        + body_start;
    generated[body_start..body_end]
        .lines()
        .map(|line| line.trim().trim_end_matches(',').to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn message_tables_match_wayland_scanner() {
    let generated = generated();
    // (message table entry, the `wayland-scanner` line it reproduces)
    //
    // Chosen to cover every encoding rule at once: plain arguments, a `since`
    // prefix with and without arguments, nullable objects, file descriptors,
    // arrays, typed `new_id`s, and the three-slot untyped `new_id`.
    let expected = [
        // { "sync", "n", wayland_types + 8 }
        (
            r#"c"sync", c"n", core::ptr::from_ref(&super::TYPES[8])"#,
            "wl_display.sync",
        ),
        (
            r#"c"get_registry", c"n", core::ptr::from_ref(&super::TYPES[9])"#,
            "wl_display.get_registry",
        ),
        (
            r#"c"error", c"ous", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_display.error",
        ),
        // The untyped new_id: one XML argument, three wire slots.
        (
            r#"c"bind", c"usun", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_registry.bind",
        ),
        (
            r#"c"global", c"usu", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_registry.global",
        ),
        (
            r#"c"create_surface", c"n", core::ptr::from_ref(&super::TYPES[10])"#,
            "wl_compositor.create_surface",
        ),
        // A `since` with no arguments at all.
        (
            r#"c"release", c"7", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_compositor.release",
        ),
        // A file descriptor argument.
        (
            r#"c"create_pool", c"nhi", core::ptr::from_ref(&super::TYPES[18])"#,
            "wl_shm.create_pool",
        ),
        (
            r#"c"create_buffer", c"niiiiu", core::ptr::from_ref(&super::TYPES[12])"#,
            "wl_shm_pool.create_buffer",
        ),
        // A nullable object in the middle of the arguments.
        (
            r#"c"attach", c"?oii", core::ptr::from_ref(&super::TYPES[58])"#,
            "wl_surface.attach",
        ),
        (
            r#"c"frame", c"n", core::ptr::from_ref(&super::TYPES[61])"#,
            "wl_surface.frame",
        ),
        (
            r#"c"set_opaque_region", c"?o", core::ptr::from_ref(&super::TYPES[62])"#,
            "wl_surface.set_opaque_region",
        ),
        // `since` prefixes on requests and on events.
        (
            r#"c"set_buffer_scale", c"3i", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_surface.set_buffer_scale",
        ),
        (
            r#"c"damage_buffer", c"4iiii", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_surface.damage_buffer",
        ),
        (
            r#"c"get_release", c"7n", core::ptr::from_ref(&super::TYPES[64])"#,
            "wl_surface.get_release",
        ),
        (
            r#"c"enter", c"o", core::ptr::from_ref(&super::TYPES[65])"#,
            "wl_surface.enter",
        ),
        (
            r#"c"preferred_buffer_scale", c"6i", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_surface.preferred_buffer_scale",
        ),
        // The widest all-null message in wayland.xml — it is what sets the
        // length of the shared leading null run.
        (
            r#"c"geometry", c"iiiiissi", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_output.geometry",
        ),
        (
            r#"c"mode", c"uiii", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_output.mode",
        ),
        (
            r#"c"done", c"2", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_output.done",
        ),
        (
            r#"c"name", c"4s", core::ptr::from_ref(&super::TYPES[0])"#,
            "wl_output.name",
        ),
        (
            r#"c"set_cursor", c"u?oii", core::ptr::from_ref(&super::TYPES[70])"#,
            "wl_pointer.set_cursor",
        ),
        (
            r#"c"enter", c"uoff", core::ptr::from_ref(&super::TYPES[74])"#,
            "wl_pointer.enter",
        ),
        (
            r#"c"leave", c"uo", core::ptr::from_ref(&super::TYPES[78])"#,
            "wl_pointer.leave",
        ),
        // An array argument.
        (
            r#"c"enter", c"uoa", core::ptr::from_ref(&super::TYPES[80])"#,
            "wl_keyboard.enter",
        ),
        // xdg-shell, whose null run is 4 rather than 8.
        (
            r#"c"create_positioner", c"n", core::ptr::from_ref(&super::TYPES[4])"#,
            "xdg_wm_base.create_positioner",
        ),
        (
            r#"c"get_xdg_surface", c"no", core::ptr::from_ref(&super::TYPES[5])"#,
            "xdg_wm_base.get_xdg_surface",
        ),
        (
            r#"c"get_toplevel", c"n", core::ptr::from_ref(&super::TYPES[7])"#,
            "xdg_surface.get_toplevel",
        ),
        (
            r#"c"get_popup", c"n?oo", core::ptr::from_ref(&super::TYPES[8])"#,
            "xdg_surface.get_popup",
        ),
        (
            r#"c"ack_configure", c"u", core::ptr::from_ref(&super::TYPES[0])"#,
            "xdg_surface.ack_configure",
        ),
        (
            r#"c"show_window_menu", c"ouii", core::ptr::from_ref(&super::TYPES[12])"#,
            "xdg_toplevel.show_window_menu",
        ),
        (
            r#"c"move", c"ou", core::ptr::from_ref(&super::TYPES[16])"#,
            "xdg_toplevel.move",
        ),
        (
            r#"c"resize", c"ouu", core::ptr::from_ref(&super::TYPES[18])"#,
            "xdg_toplevel.resize",
        ),
        (
            r#"c"set_fullscreen", c"?o", core::ptr::from_ref(&super::TYPES[21])"#,
            "xdg_toplevel.set_fullscreen",
        ),
        (
            r#"c"configure", c"iia", core::ptr::from_ref(&super::TYPES[0])"#,
            "xdg_toplevel.configure",
        ),
        (
            r#"c"wm_capabilities", c"5a", core::ptr::from_ref(&super::TYPES[0])"#,
            "xdg_toplevel.wm_capabilities",
        ),
        (
            r#"c"reposition", c"3ou", core::ptr::from_ref(&super::TYPES[24])"#,
            "xdg_popup.reposition",
        ),
    ];
    for (needle, message) in expected {
        assert!(
            generated.contains(needle),
            "{message} does not match wayland-scanner: expected to find\n  {needle}"
        );
    }
}

#[test]
fn the_wayland_type_table_matches_wayland_scanner_entry_for_entry() {
    let generated = generated();
    let table = type_table(&generated, "wayland");

    // `wayland-scanner` emits eight leading NULLs, one for each argument of the
    // widest message that names no interface (`wl_output.geometry`). Every
    // all-null message points at offset zero, so this run is shared.
    assert_eq!(table.len(), 97, "wayland_types[] has 97 entries");
    for (index, entry) in table.iter().take(8).enumerate() {
        assert_eq!(
            entry, "crate::wayland::ffi::WlInterfacePtr::NULL",
            "slot {index}"
        );
    }

    let named = |path: &str| {
        format!("crate::wayland::ffi::WlInterfacePtr::new(&super::wayland::{path}::INTERFACE)")
    };
    // Spot-checks straight off `wayland-scanner`'s table.
    assert_eq!(table[8], named("wl_callback"), "wl_display.sync");
    assert_eq!(table[9], named("wl_registry"), "wl_display.get_registry");
    assert_eq!(
        table[10],
        named("wl_surface"),
        "wl_compositor.create_surface"
    );
    assert_eq!(table[11], named("wl_region"), "wl_compositor.create_region");
    assert_eq!(table[12], named("wl_buffer"), "wl_shm_pool.create_buffer");
    for entry in &table[13..18] {
        assert_eq!(
            entry, "crate::wayland::ffi::WlInterfacePtr::NULL",
            "the five scalar arguments of wl_shm_pool.create_buffer"
        );
    }
    assert_eq!(table[18], named("wl_shm_pool"), "wl_shm.create_pool");
    assert_eq!(table[58], named("wl_buffer"), "wl_surface.attach");
    assert_eq!(table[61], named("wl_callback"), "wl_surface.frame");
    assert_eq!(table[65], named("wl_output"), "wl_surface.enter");
    assert_eq!(table[96], named("wl_registry"), "wl_fixes.destroy_registry");
}

#[test]
fn the_xdg_shell_type_table_reaches_into_the_core_protocol() {
    let generated = generated();
    let table = type_table(&generated, "xdg_shell");
    assert_eq!(table.len(), 26, "xdg_shell_types[] has 26 entries");
    for entry in table.iter().take(4) {
        assert_eq!(entry, "crate::wayland::ffi::WlInterfacePtr::NULL");
    }
    let xdg = |path: &str| {
        format!("crate::wayland::ffi::WlInterfacePtr::new(&super::xdg_shell::{path}::INTERFACE)")
    };
    let core = |path: &str| {
        format!("crate::wayland::ffi::WlInterfacePtr::new(&super::wayland::{path}::INTERFACE)")
    };
    assert_eq!(table[4], xdg("xdg_positioner"));
    assert_eq!(table[5], xdg("xdg_surface"));
    // The cross-protocol reference: this is the entry that would be a null
    // pointer if the generator resolved interfaces per file.
    assert_eq!(table[6], core("wl_surface"), "xdg_wm_base.get_xdg_surface");
    assert_eq!(table[7], xdg("xdg_toplevel"));
    assert_eq!(table[12], core("wl_seat"), "xdg_toplevel.show_window_menu");
    assert_eq!(table[21], core("wl_output"), "xdg_toplevel.set_fullscreen");
}

#[test]
fn interface_descriptors_carry_the_versions_and_counts_scanner_emits() {
    let generated = generated();
    for needle in [
        // "wl_display", 1, 2 requests, 2 events
        r#"pub const NAME: &core::ffi::CStr = c"wl_display";"#,
        r#"pub const NAME: &core::ffi::CStr = c"xdg_wm_base";"#,
        "static REQUESTS: [crate::wayland::ffi::WlMessage; 14] = [", // xdg_toplevel
        "static EVENTS: [crate::wayland::ffi::WlMessage; 11] = [",   // wl_pointer
        "static EVENTS: [crate::wayland::ffi::WlMessage; 6] = [",    // wl_output
    ] {
        assert!(generated.contains(needle), "missing: {needle}");
    }
    // Interface versions, which decide what a `bind` may ask for.
    assert!(
        generated.contains("pub const VERSION: u32 = 7;"),
        "xdg_wm_base is v7"
    );
    assert!(
        generated.contains("pub const VERSION: u32 = 10;"),
        "wl_seat is v10"
    );
}

#[test]
fn opcodes_are_declaration_order() {
    let generated = generated();
    // Getting an opcode wrong sends the right arguments to the wrong request,
    // which a compositor answers with a protocol error and a disconnect.
    for needle in [
        "pub const REQ_GET_XDG_SURFACE: u32 = 2;",
        "pub const REQ_PONG: u32 = 3;",
        "pub const EVT_PING: u32 = 0;",
        "pub const REQ_ACK_CONFIGURE: u32 = 4;",
        "pub const EVT_CONFIGURE: u32 = 0;",
        "pub const REQ_SET_FULLSCREEN: u32 = 11;",
        "pub const REQ_UNSET_FULLSCREEN: u32 = 12;",
        "pub const REQ_SET_BUFFER_SCALE: u32 = 8;",
        "pub const REQ_COMMIT: u32 = 6;",
    ] {
        assert!(generated.contains(needle), "missing: {needle}");
    }
}

#[test]
fn enums_and_keyword_collisions_survive_the_trip() {
    let generated = generated();
    // `xdg_toplevel.state.fullscreen` is what the backend reads to decide
    // whether a borderless request was honoured.
    assert!(generated.contains("pub const FULLSCREEN: u32 = 2;"));
    assert!(generated.contains("pub const ACTIVATED: u32 = 4;"));
    // `wl_output.transform` has entries literally named `90`.
    assert!(generated.contains("pub const _90: u32 = 1;"));
    // `xdg_toplevel.move` is a Rust keyword.
    assert!(generated.contains("pub unsafe fn r#move("));
}

#[test]
fn request_wrappers_and_event_decoders_are_generated_for_the_slice_we_need() {
    let generated = generated();
    for needle in [
        "pub unsafe fn get_registry(proxy: *mut crate::wayland::ffi::WlProxy) -> *mut crate::wayland::ffi::WlProxy",
        // The untyped `new_id` takes the interface and version from the caller.
        "pub unsafe fn bind(proxy: *mut crate::wayland::ffi::WlProxy, name: u32, interface: &'static crate::wayland::ffi::WlInterface, version: u32) -> *mut crate::wayland::ffi::WlProxy",
        "pub unsafe fn set_title(proxy: *mut crate::wayland::ffi::WlProxy, title: &core::ffi::CStr)",
        // A nullable object argument stays a raw pointer the caller may null.
        "pub unsafe fn set_fullscreen(proxy: *mut crate::wayland::ffi::WlProxy, output: *mut crate::wayland::ffi::WlProxy)",
        "pub unsafe fn decode_event<'a>(opcode: u32, args: *const crate::wayland::ffi::WlArgument) -> Option<Event<'a>>",
    ] {
        assert!(generated.contains(needle), "missing: {needle}");
    }
    // A destructor request carries WL_MARSHAL_FLAG_DESTROY; a plain one does
    // not. Losing the flag leaks the proxy and desynchronises object ids.
    let destroy = generated
        .find("pub unsafe fn destroy(proxy")
        .expect("wl_display has no destroy, but wl_registry does");
    assert!(
        generated[destroy..destroy + 900].contains("crate::wayland::ffi::MARSHAL_FLAG_DESTROY"),
        "a destructor must marshal with WL_MARSHAL_FLAG_DESTROY"
    );
}

/// The protocols P0.5b added, pinned the same way: against
/// `wayland-scanner private-code` on the same vendored XML.
///
/// These are the ones whose type tables are *not* trivial — every one of them
/// points across a protocol boundary into `wayland.xml` or `xdg-shell.xml`, and
/// a generator that resolved interfaces per file would emit a `NULL` there. A
/// `NULL` in a `new_id`'s type slot is not a compile error and not a protocol
/// error: libwayland encodes the request against the wrong descriptor and the
/// compositor reads garbage.
#[test]
fn the_p05b_protocol_tables_match_wayland_scanner() {
    let generated = generated();
    // (message table entry, the `wayland-scanner` line it reproduces)
    let expected = [
        // viewporter_types[] = { NULL, NULL, NULL, NULL, &wp_viewport, &wl_surface }
        (
            r#"c"get_viewport", c"no", core::ptr::from_ref(&super::TYPES[4])"#,
            "wp_viewporter.get_viewport",
        ),
        (
            r#"c"set_destination", c"ii", core::ptr::from_ref(&super::TYPES[0])"#,
            "wp_viewport.set_destination",
        ),
        (
            r#"c"set_source", c"ffff", core::ptr::from_ref(&super::TYPES[0])"#,
            "wp_viewport.set_source",
        ),
        // fractional_scale_v1_types[] = { NULL, &wp_fractional_scale_v1, &wl_surface }
        (
            r#"c"get_fractional_scale", c"no", core::ptr::from_ref(&super::TYPES[1])"#,
            "wp_fractional_scale_manager_v1.get_fractional_scale",
        ),
        (
            r#"c"preferred_scale", c"u", core::ptr::from_ref(&super::TYPES[0])"#,
            "wp_fractional_scale_v1.preferred_scale",
        ),
        // xdg_output_unstable_v1_types[] = { NULL, NULL, &zxdg_output_v1, &wl_output }
        (
            r#"c"get_xdg_output", c"no", core::ptr::from_ref(&super::TYPES[2])"#,
            "zxdg_output_manager_v1.get_xdg_output",
        ),
        (
            r#"c"logical_position", c"ii", core::ptr::from_ref(&super::TYPES[0])"#,
            "zxdg_output_v1.logical_position",
        ),
        // A `since` prefix on an event of an unstable protocol.
        (
            r#"c"name", c"2s", core::ptr::from_ref(&super::TYPES[0])"#,
            "zxdg_output_v1.name",
        ),
        // xdg_decoration_unstable_v1_types[] = { NULL, &zxdg_toplevel_decoration_v1, &xdg_toplevel }
        (
            r#"c"get_toplevel_decoration", c"no", core::ptr::from_ref(&super::TYPES[1])"#,
            "zxdg_decoration_manager_v1.get_toplevel_decoration",
        ),
        // relative_pointer_unstable_v1_types[] has a six-NULL run, because
        // `relative_motion`'s "uuffff" is the widest all-null message.
        (
            r#"c"get_relative_pointer", c"no", core::ptr::from_ref(&super::TYPES[6])"#,
            "zwp_relative_pointer_manager_v1.get_relative_pointer",
        ),
        (
            r#"c"relative_motion", c"uuffff", core::ptr::from_ref(&super::TYPES[0])"#,
            "zwp_relative_pointer_v1.relative_motion",
        ),
        // The most interesting one: a typed `new_id`, two objects, a *nullable*
        // object and a uint, twice over, into a table with a two-NULL run.
        (
            r#"c"lock_pointer", c"noo?ou", core::ptr::from_ref(&super::TYPES[2])"#,
            "zwp_pointer_constraints_v1.lock_pointer",
        ),
        (
            r#"c"confine_pointer", c"noo?ou", core::ptr::from_ref(&super::TYPES[7])"#,
            "zwp_pointer_constraints_v1.confine_pointer",
        ),
        (
            r#"c"set_cursor_position_hint", c"ff", core::ptr::from_ref(&super::TYPES[0])"#,
            "zwp_locked_pointer_v1.set_cursor_position_hint",
        ),
        (
            r#"c"set_region", c"?o", core::ptr::from_ref(&super::TYPES[12])"#,
            "zwp_locked_pointer_v1.set_region",
        ),
    ];
    for (needle, message) in expected {
        assert!(
            generated.contains(needle),
            "{message} does not match wayland-scanner: expected to find\n  {needle}"
        );
    }
}

/// `pointer-constraints`' table, entry for entry against
/// `pointer_constraints_unstable_v1_types[]`.
///
/// Chosen over the other five because it is the only one with more than one
/// row *and* a cross-protocol reference in the middle of a row: getting the
/// null run wrong by one would still produce a plausible-looking table with
/// `wl_pointer` where `wl_surface` belongs, and `lock_pointer` would then send
/// the pointer as the surface.
#[test]
fn the_pointer_constraints_type_table_matches_wayland_scanner_entry_for_entry() {
    let generated = generated();
    let table = type_table(&generated, "pointer_constraints");
    let null = "crate::wayland::ffi::WlInterfacePtr::NULL";
    let named = |module: &str, path: &str| {
        format!("crate::wayland::ffi::WlInterfacePtr::new(&super::{module}::{path}::INTERFACE)")
    };

    assert_eq!(
        table.len(),
        14,
        "pointer_constraints_unstable_v1_types[] has 14 entries"
    );
    // The shared leading null run is two long: `set_cursor_position_hint`'s
    // "ff" is the widest message naming no interface.
    assert_eq!(table[0], null);
    assert_eq!(table[1], null);
    // lock_pointer: new_id, surface, pointer, ?region, uint
    assert_eq!(
        table[2],
        named("pointer_constraints", "zwp_locked_pointer_v1")
    );
    assert_eq!(table[3], named("wayland", "wl_surface"));
    assert_eq!(table[4], named("wayland", "wl_pointer"));
    assert_eq!(table[5], named("wayland", "wl_region"));
    assert_eq!(table[6], null, "the lifetime enum names no interface");
    // confine_pointer, same shape.
    assert_eq!(
        table[7],
        named("pointer_constraints", "zwp_confined_pointer_v1")
    );
    assert_eq!(table[8], named("wayland", "wl_surface"));
    assert_eq!(table[9], named("wayland", "wl_pointer"));
    assert_eq!(table[10], named("wayland", "wl_region"));
    assert_eq!(table[11], null);
    // The two `set_region` requests, one per constraint interface.
    assert_eq!(table[12], named("wayland", "wl_region"));
    assert_eq!(table[13], named("wayland", "wl_region"));
}

/// The versions and enum values the backend binds and compares against.
#[test]
fn the_p05b_interface_versions_and_enums_survive_the_trip() {
    let generated = generated();
    for needle in [
        r#"pub const NAME: &core::ffi::CStr = c"zwp_pointer_constraints_v1";"#,
        r#"pub const NAME: &core::ffi::CStr = c"zwp_relative_pointer_manager_v1";"#,
        r#"pub const NAME: &core::ffi::CStr = c"zxdg_output_manager_v1";"#,
        r#"pub const NAME: &core::ffi::CStr = c"zxdg_decoration_manager_v1";"#,
        r#"pub const NAME: &core::ffi::CStr = c"wp_fractional_scale_manager_v1";"#,
        r#"pub const NAME: &core::ffi::CStr = c"wp_viewporter";"#,
        // `zwp_pointer_constraints_v1.lifetime`, which `set_pointer_mode`
        // passes: `oneshot` would silently lose the constraint on the first
        // alt-tab.
        "pub const PERSISTENT: u32 = 2;",
        // `zxdg_toplevel_decoration_v1.mode`, which `create_window` requests.
        "pub const SERVER_SIDE: u32 = 2;",
        // `wl_pointer.axis_value120`, the reason `wl_seat` is bound at 8.
        "pub const EVT_AXIS_VALUE120: u32 = 9;",
        // `wl_keyboard.keymap_format.xkb_v1`, the format the keymap arrives in.
        "pub const XKB_V1: u32 = 1;",
    ] {
        assert!(generated.contains(needle), "missing: {needle}");
    }
    // The unstable protocols are all at the version the vendored XML declares.
    assert!(
        generated.contains("pub const VERSION: u32 = 3;"),
        "zxdg_output_manager_v1 is v3"
    );
}
