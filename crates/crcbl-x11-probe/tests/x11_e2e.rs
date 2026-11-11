//! `crcbl_x11::Connection` against a real X server.
//!
//! Behind the `x11-e2e` feature and `#[ignore]`d, because it needs one:
//! `crates/crcbl-shell/tests/run-x11-e2e.sh` runs it against its Xvfb, one test
//! at a time, because the tests change the server's configuration — a mode, the
//! screen size, an output property — and put it back.
//!
//! Xvfb's RandR has one CRTC driving one output, and a driver that cannot pan
//! or transform; the tests that touch those accept either the round trip or
//! the typed refusal Xvfb gives, and assert the refusal's every field.

#![cfg(any(target_os = "linux", target_os = "freebsd"))]

use crcbl_x11::{
    Connection, Crtc, CrtcConfig, CrtcInfo, Error, Event, ModeInfo, NotifyMask, Output,
    OutputConnection, PropertyMode, PropertyState, PropertyValue, RandrErrorKind, RandrVersion,
    Rotation, ScreenResources, SetConfigStatus, Transform, Window, mode_flag,
};

/// `XCB_ATOM_INTEGER`.
const ATOM_INTEGER: u32 = 19;
/// `XCB_ATOM_ATOM`.
const ATOM_ATOM: u32 = 4;
/// Core `BadValue`.
const BAD_VALUE: u8 = 2;
/// Core `BadMatch`.
const BAD_MATCH: u8 = 8;
/// Core `BadDrawable`.
const BAD_DRAWABLE: u8 = 9;
/// Core `GetGeometry`'s major opcode, from the X protocol.
const GET_GEOMETRY: u8 = 14;
/// RandR minor opcodes, from `randrproto.txt`.
const GET_OUTPUT_INFO: u16 = 9;
const DESTROY_MODE: u16 = 17;
const GET_CRTC_INFO: u16 = 20;
const SET_CRTC_TRANSFORM: u16 = 26;
const SET_PANNING: u16 = 29;
/// An id no CRTC, output or mode has: the highest legal XID, which on a
/// server with the default client limit is inside client 255's range — a
/// client slot no harness run gets near.
const BOGUS: u32 = 0x1fff_ffff;

fn connect() -> Connection {
    let connection = Connection::connect(None).expect("DISPLAY names a running X server");
    connection.randr().expect("the server has RandR");
    connection
}

/// Every event the server has sent so far: a round trip first, so the
/// events every earlier request caused are in the queue.
fn drain(connection: &Connection) -> Vec<Event> {
    connection.sync().expect("the round trip");
    let mut events = Vec::new();
    while let Some(event) = connection.poll_event().expect("the connection is alive") {
        events.push(event);
    }
    events
}

/// The first CRTC that is on, with what it is set to.
fn lit_crtc(connection: &Connection, resources: &ScreenResources) -> (Crtc, CrtcInfo) {
    resources
        .crtcs
        .iter()
        .map(|&crtc| {
            let info = connection
                .crtc_info(crtc, resources.config_timestamp)
                .expect("a CRTC the resources list");
            (crtc, info)
        })
        .find(|(_, info)| info.mode != 0 && !info.outputs.is_empty())
        .expect("the server drives at least one CRTC")
}

#[test]
#[ignore = "needs an X server with RandR; run-x11-e2e.sh provides one"]
fn randr_is_1_5_and_every_read_agrees_with_the_others() {
    let connection = connect();
    let randr = connection.randr().expect("RandR");
    assert!(
        randr.version >= RandrVersion::REQUESTED,
        "the server settles on 1.5 or newer: {}",
        randr.version
    );
    assert_eq!(
        connection.query_version().expect("QueryVersion"),
        randr.version
    );

    let screen = connection.screen();
    let root = screen.root;
    let range = connection.screen_size_range(root).expect("the size range");
    assert!(
        (range.min_width..=range.max_width).contains(&screen.width)
            && (range.min_height..=range.max_height).contains(&screen.height),
        "the screen's size is inside its range: {screen:?} {range:?}"
    );

    let resources = connection
        .screen_resources_current(root)
        .expect("the resources");
    assert!(!resources.crtcs.is_empty() && !resources.outputs.is_empty());
    // The probing request finds nothing new on Xvfb, whose one output is
    // always connected: it agrees with the cached one.
    let probed = connection
        .screen_resources(root)
        .expect("the probed resources");
    assert_eq!(
        (&probed.crtcs, &probed.outputs, &probed.modes),
        (&resources.crtcs, &resources.outputs, &resources.modes)
    );
    let mode_ids: Vec<u32> = resources.modes.iter().map(|mode| mode.id).collect();
    for mode in &resources.modes {
        assert!(!mode.name.is_empty(), "every mode is named: {mode:?}");
    }

    for &output in &resources.outputs {
        let info = connection
            .output_info(output, resources.config_timestamp)
            .expect("an output the resources list");
        assert!(!info.name.is_empty(), "every output is named: {info:?}");
        assert!(info.crtc == 0 || resources.crtcs.contains(&info.crtc));
        assert!(info.crtcs.iter().all(|crtc| resources.crtcs.contains(crtc)));
        assert!(info.modes.iter().all(|mode| mode_ids.contains(mode)));
        assert!(usize::from(info.num_preferred) <= info.modes.len());
        assert!(
            info.clones
                .iter()
                .all(|clone| resources.outputs.contains(clone))
        );
    }

    for &crtc in &resources.crtcs {
        let info = connection
            .crtc_info(crtc, resources.config_timestamp)
            .expect("a CRTC the resources list");
        assert!(
            info.possible
                .iter()
                .all(|output| resources.outputs.contains(output))
        );
        assert!(
            info.outputs
                .iter()
                .all(|output| info.possible.contains(output))
        );
        assert!(
            info.rotations.contains(info.rotation),
            "a CRTC is at a rotation it can do: {info:?}"
        );
        for &output in &info.outputs {
            let output = connection
                .output_info(output, resources.config_timestamp)
                .expect("an output a CRTC drives");
            assert_eq!(
                output.crtc, crtc,
                "the output names the CRTC that drives it"
            );
        }
        if info.mode != 0 {
            let mode = resources
                .modes
                .iter()
                .find(|mode| mode.id == info.mode)
                .expect("a lit CRTC's mode is in the resources");
            if !info.rotation.contains(Rotation::ROTATE_90)
                && !info.rotation.contains(Rotation::ROTATE_270)
            {
                assert_eq!((info.width, info.height), (mode.width, mode.height));
            }
        }
        connection
            .crtc_gamma_size(crtc)
            .expect("every CRTC has a gamma size");
    }

    let primary = connection.output_primary(root).expect("the primary output");
    assert!(primary == 0 || resources.outputs.contains(&primary));

    let monitors = connection.monitors(root, true).expect("the monitors");
    assert!(!monitors.monitors.is_empty(), "a lit CRTC is a monitor");
    for monitor in &monitors.monitors {
        assert!(
            !connection
                .atom_name(monitor.name)
                .expect("a monitor's name is an atom")
                .is_empty()
        );
        for &output in &monitor.outputs {
            assert!(resources.outputs.contains(&output));
            let crtc = connection
                .output_info(output, resources.config_timestamp)
                .expect("a monitor's output")
                .crtc;
            let crtc = connection
                .crtc_info(crtc, resources.config_timestamp)
                .expect("the CRTC a monitor's output is on");
            assert_eq!(
                (monitor.x, monitor.y, monitor.width, monitor.height),
                (crtc.x, crtc.y, crtc.width, crtc.height),
                "an automatic monitor covers its CRTC"
            );
        }
    }

    connection.sync().expect("the round trip");
    assert_eq!(connection.has_error(), None);
}

/// Millimetres for `pixels` of a screen dimension that is `total` pixels and
/// `mm` millimetres long, so a resized screen keeps its physical density.
fn scaled_mm(pixels: u16, total: u16, mm: u16) -> u32 {
    u32::from(mm) * u32::from(pixels) / u32::from(total).max(1)
}

/// Sets the screen back to its connect-time size, then lights `crtc` again as
/// `before` described it, and says which step refused. Never panics: it also
/// runs while a failed test unwinds, where a second panic would abort before
/// the rest was put back.
fn restore_size_and_crtc(
    connection: &Connection,
    crtc: Crtc,
    before: &CrtcInfo,
) -> Result<(), String> {
    let refused = |step: &'static str| move |error: Error| format!("{step}: {error}");
    let screen = connection.screen();
    connection
        .set_screen_size(
            screen.root,
            screen.width,
            screen.height,
            u32::from(screen.width_mm),
            u32::from(screen.height_mm),
        )
        .map_err(refused("SetScreenSize"))?;
    let resources = connection
        .screen_resources_current(screen.root)
        .map_err(refused("GetScreenResourcesCurrent"))?;
    let reply = connection
        .set_crtc_config(
            crtc,
            0,
            resources.config_timestamp,
            &CrtcConfig {
                x: before.x,
                y: before.y,
                mode: before.mode,
                rotation: before.rotation,
                outputs: &before.outputs,
            },
        )
        .map_err(refused("SetCrtcConfig"))?;
    if reply.status == SetConfigStatus::Success {
        Ok(())
    } else {
        Err(format!("SetCrtcConfig answered {:?}", reply.status))
    }
}

/// Puts the screen and CRTC back if the test that changed them panics, so a
/// failure here does not leave the harness's later steps a different screen.
struct Restore<'a> {
    connection: &'a Connection,
    crtc: Crtc,
    before: CrtcInfo,
    output: Output,
    mode: u32,
}

impl Restore<'_> {
    /// Puts everything back, and says which step refused. Never panics: it
    /// also runs while a failed test unwinds, where a second panic would
    /// abort before the rest was put back.
    fn restore(&self) -> Result<(), String> {
        let refused = |step: &'static str| move |error: Error| format!("{step}: {error}");
        let connection = self.connection;
        restore_size_and_crtc(connection, self.crtc, &self.before)?;
        connection
            .delete_output_mode(self.output, self.mode)
            .map_err(refused("DeleteOutputMode"))?;
        connection
            .destroy_mode(self.mode)
            .map_err(refused("DestroyMode"))
    }
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        if std::thread::panicking()
            && let Err(error) = self.restore()
        {
            eprintln!("crcbl e2e: restoring the screen after a failure: {error}");
        }
    }
}

#[test]
#[ignore = "needs an X server with RandR; run-x11-e2e.sh provides one"]
fn a_new_mode_drives_the_crtc_and_the_events_report_it() {
    let connection = connect();
    let screen = connection.screen();
    let root = screen.root;
    connection
        .select_input(
            root,
            NotifyMask::SCREEN_CHANGE | NotifyMask::CRTC_CHANGE | NotifyMask::OUTPUT_CHANGE,
        )
        .expect("RandR events on the root");
    drain(&connection);

    let resources = connection
        .screen_resources_current(root)
        .expect("the resources");
    let (crtc, before) = lit_crtc(&connection, &resources);
    let output = before.outputs[0];

    // VESA's 1024x768 at 60 Hz: smaller than any screen the harness starts,
    // so the screen can shrink to it.
    let wanted = ModeInfo {
        width: 1024,
        height: 768,
        dot_clock: 65_000_000,
        hsync_start: 1048,
        hsync_end: 1184,
        htotal: 1344,
        vsync_start: 771,
        vsync_end: 777,
        vtotal: 806,
        flags: mode_flag::HSYNC_NEGATIVE | mode_flag::VSYNC_NEGATIVE,
        name: format!("crcbl-probe-{}", std::process::id()),
        ..ModeInfo::default()
    };
    let mode = connection.create_mode(root, &wanted).expect("CreateMode");
    connection
        .add_output_mode(output, mode)
        .expect("AddOutputMode");
    let restore = Restore {
        connection: &connection,
        crtc,
        before: before.clone(),
        output,
        mode,
    };

    let resources = connection
        .screen_resources_current(root)
        .expect("the resources");
    let created = resources
        .modes
        .iter()
        .find(|info| info.id == mode)
        .expect("the new mode is in the resources");
    assert_eq!(
        *created,
        ModeInfo {
            id: mode,
            ..wanted.clone()
        },
        "the mode reads back as created, its name split from the names block"
    );
    assert!(
        connection
            .output_info(output, resources.config_timestamp)
            .expect("the output")
            .modes
            .contains(&mode)
    );

    let config = CrtcConfig {
        x: 0,
        y: 0,
        mode,
        rotation: Rotation::ROTATE_0,
        outputs: &[output],
    };
    let applied = connection
        .set_crtc_config(crtc, 0, resources.config_timestamp, &config)
        .expect("SetCrtcConfig");
    assert_eq!(applied.status, SetConfigStatus::Success);
    // The X.Org server checks neither of `SetCrtcConfig`'s timestamps — it
    // records whatever `timestamp` it is sent — so the two time statuses
    // cannot be provoked here; `SetConfigStatus`'s unit test maps them.

    connection
        .set_screen_size(
            root,
            1024,
            768,
            scaled_mm(1024, screen.width, screen.width_mm),
            scaled_mm(768, screen.height, screen.height_mm),
        )
        .expect("SetScreenSize to the CRTC's size");

    let crtc_now = connection
        .crtc_info(crtc, resources.config_timestamp)
        .expect("the CRTC");
    assert_eq!(
        (crtc_now.mode, crtc_now.width, crtc_now.height),
        (mode, 1024, 768)
    );
    assert_eq!(
        connection
            .screen_resources_current(root)
            .expect("the resources")
            .timestamp,
        applied.timestamp,
        "the resources carry the set's timestamp"
    );

    let events = drain(&connection);
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::CrtcChange(change)
                if change.crtc == crtc
                    && change.mode == mode
                    && (change.width, change.height) == (1024, 768)
                    && change.rotation == Rotation::ROTATE_0
        )),
        "CrtcChange for the new mode: {events:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::ScreenChange(change)
                if change.root == root && (change.width, change.height) == (1024, 768)
        )),
        "ScreenChangeNotify for the new size: {events:?}"
    );
    // The X.Org server sends `OutputChange` when the output itself changes —
    // `AddOutputMode` changed its mode list — not when its CRTC changes mode.
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::OutputChange(change)
                if change.output == output
                    && change.crtc == crtc
                    && change.connection == OutputConnection::Connected
        )),
        "OutputChange for the output given a mode: {events:?}"
    );

    restore.restore().expect("the screen and CRTC go back");
    let crtc_after = connection
        .crtc_info(crtc, 0)
        .expect("the CRTC after restoring");
    assert_eq!(
        (
            crtc_after.mode,
            crtc_after.x,
            crtc_after.y,
            crtc_after.width,
            crtc_after.height
        ),
        (before.mode, before.x, before.y, before.width, before.height)
    );
    assert!(
        !connection
            .screen_resources_current(root)
            .expect("the resources")
            .modes
            .iter()
            .any(|info| info.id == mode),
        "the destroyed mode is gone"
    );
    let events = drain(&connection);
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::ScreenChange(change)
                if (change.width, change.height) == (screen.width, screen.height)
        )),
        "ScreenChangeNotify for the restored size: {events:?}"
    );
}

#[test]
#[ignore = "needs an X server with RandR; run-x11-e2e.sh provides one"]
fn an_output_property_is_set_extended_queried_and_deleted() {
    let connection = connect();
    let root = connection.screen().root;
    let resources = connection
        .screen_resources_current(root)
        .expect("the resources");
    let output = resources.outputs[0];

    let name = "CRCBL_PROBE_PROPERTY";
    let atom = connection.intern_atom(name, false).expect("InternAtom");
    assert_ne!(atom, 0);
    assert_eq!(connection.atom_name(atom).expect("GetAtomName"), name);
    assert_eq!(
        connection
            .intern_atom(&format!("CRCBL_PROBE_NEVER_{}", std::process::id()), true)
            .expect("InternAtom"),
        0,
        "only_if_exists does not create the atom"
    );

    connection
        .select_input(root, NotifyMask::OUTPUT_PROPERTY)
        .expect("RandR property events on the root");
    drain(&connection);

    let read = |type_| {
        connection
            .get_output_property(output, atom, type_, 0, 16, false, false)
            .expect("GetOutputProperty")
    };
    let set = |mode, value: Vec<u32>| {
        connection
            .change_output_property(
                output,
                atom,
                ATOM_INTEGER,
                mode,
                &PropertyValue::Format32(value),
            )
            .expect("ChangeOutputProperty");
    };
    set(PropertyMode::Replace, vec![7]);
    assert!(
        connection
            .list_output_properties(output)
            .expect("ListOutputProperties")
            .contains(&atom)
    );
    let value = read(0);
    assert_eq!(
        (value.type_, value.bytes_after, value.value),
        (ATOM_INTEGER, 0, Some(PropertyValue::Format32(vec![7])))
    );
    set(PropertyMode::Append, vec![8]);
    set(PropertyMode::Prepend, vec![6]);
    assert_eq!(read(0).value, Some(PropertyValue::Format32(vec![6, 7, 8])));

    // A read shorter than the value: `long_offset` and `long_length` count
    // 4-byte units, and what is left is said in bytes.
    let part = |long_offset, long_length| {
        let part = connection
            .get_output_property(output, atom, 0, long_offset, long_length, false, false)
            .expect("GetOutputProperty");
        (part.bytes_after, part.value)
    };
    assert_eq!(part(0, 1), (8, Some(PropertyValue::Format32(vec![6]))));
    assert_eq!(part(1, 1), (4, Some(PropertyValue::Format32(vec![7]))));
    assert_eq!(part(1, 16), (0, Some(PropertyValue::Format32(vec![7, 8]))));

    // A type that does not match returns the real type and size, and no
    // items. The size is the X.Org server's: three items, where the protocol
    // says twelve bytes — pinned so a change in it is seen.
    let mismatched = read(ATOM_ATOM);
    assert_eq!(
        (mismatched.type_, mismatched.bytes_after, mismatched.value),
        (ATOM_INTEGER, 3, Some(PropertyValue::Format32(Vec::new())))
    );

    let info = connection
        .query_output_property(output, atom)
        .expect("QueryOutputProperty");
    assert!(!info.pending && !info.range && !info.immutable);
    assert!(info.valid_values.is_empty());

    let refused = connection
        .change_output_property(
            output,
            atom,
            ATOM_INTEGER,
            PropertyMode::Append,
            &PropertyValue::Format8(vec![1]),
        )
        .expect_err("appending another format");
    let Error::Protocol(refused) = refused else {
        panic!("a refusal is an X error: {refused:?}");
    };
    assert_eq!(refused.error_code, BAD_MATCH);

    // The other two formats, each replacing the value whole.
    let bytes = PropertyValue::Format8(vec![1, 2, 3, 4, 5]);
    connection
        .change_output_property(output, atom, ATOM_INTEGER, PropertyMode::Replace, &bytes)
        .expect("ChangeOutputProperty, format 8");
    assert_eq!(read(0).value, Some(bytes));
    assert_eq!(
        part(0, 1),
        (1, Some(PropertyValue::Format8(vec![1, 2, 3, 4]))),
        "one unit is four format-8 items, and one byte is left"
    );
    let shorts = PropertyValue::Format16(vec![1, 0x8000, 0xffff]);
    connection
        .change_output_property(output, atom, ATOM_INTEGER, PropertyMode::Replace, &shorts)
        .expect("ChangeOutputProperty, format 16");
    assert_eq!(read(0).value, Some(shorts));
    assert_eq!(
        part(0, 1),
        (2, Some(PropertyValue::Format16(vec![1, 0x8000]))),
        "one unit is two format-16 items, and two bytes are left"
    );

    connection
        .delete_output_property(output, atom)
        .expect("DeleteOutputProperty");
    let gone = read(0);
    assert_eq!((gone.type_, gone.value), (0, None));
    assert!(
        !connection
            .list_output_properties(output)
            .expect("ListOutputProperties")
            .contains(&atom)
    );

    let events = drain(&connection);
    let states: Vec<PropertyState> = events
        .iter()
        .filter_map(|event| match event {
            Event::OutputProperty(change) if change.atom == atom && change.output == output => {
                Some(change.state)
            }
            _ => None,
        })
        .collect();
    assert_eq!(states.first(), Some(&PropertyState::NewValue), "{events:?}");
    assert_eq!(states.last(), Some(&PropertyState::Deleted), "{events:?}");

    // A property the server configured: Xvfb's outputs have `non-desktop`,
    // whose valid values are the list `0, 1`.
    let non_desktop = connection
        .intern_atom("non-desktop", true)
        .expect("InternAtom");
    assert_ne!(non_desktop, 0, "the server defines non-desktop");
    let info = connection
        .query_output_property(output, non_desktop)
        .expect("QueryOutputProperty");
    assert!(!info.range && !info.immutable, "{info:?}");
    assert_eq!(info.valid_values, [0, 1]);
}

#[test]
#[ignore = "needs an X server with RandR; run-x11-e2e.sh provides one"]
fn panning_and_transforms_round_trip_or_are_refused_as_typed_errors() {
    let connection = connect();
    let randr = connection.randr().expect("RandR");
    let root = connection.screen().root;
    let resources = connection
        .screen_resources_current(root)
        .expect("the resources");
    let (crtc, _) = lit_crtc(&connection, &resources);

    let panning = connection.get_panning(crtc).expect("GetPanning");
    match connection.set_panning(crtc, &panning) {
        Ok(reply) => {
            assert_eq!(reply.status, SetConfigStatus::Success);
            let again = connection.get_panning(crtc).expect("GetPanning");
            assert_eq!(
                (again.left, again.top, again.width, again.height),
                (panning.left, panning.top, panning.width, panning.height)
            );
        }
        Err(Error::Protocol(error)) => {
            assert_eq!(
                (error.major_opcode, error.minor_opcode, error.resource_id),
                (randr.major_opcode, SET_PANNING, crtc)
            );
            assert_eq!(randr.error_kind(&error), Some(RandrErrorKind::BadCrtc));
        }
        Err(other) => panic!("SetPanning failed without an X error: {other:?}"),
    }

    let transform = connection.crtc_transform(crtc).expect("GetCrtcTransform");
    assert_eq!(transform.current, Transform::IDENTITY);
    match connection.set_crtc_transform(crtc, &Transform::IDENTITY, "", &[]) {
        Ok(()) => {
            let again = connection.crtc_transform(crtc).expect("GetCrtcTransform");
            assert_eq!(again.pending, Transform::IDENTITY);
        }
        Err(Error::Protocol(error)) => {
            assert!(!transform.has_transforms, "{transform:?} refused {error:?}");
            assert_eq!(
                (error.error_code, error.major_opcode, error.minor_opcode),
                (BAD_VALUE, randr.major_opcode, SET_CRTC_TRANSFORM)
            );
        }
        Err(other) => panic!("SetCrtcTransform failed without an X error: {other:?}"),
    }
    assert_eq!(connection.has_error(), None);
}

#[test]
#[ignore = "needs an X server with RandR; run-x11-e2e.sh provides one"]
fn ids_that_do_not_exist_are_typed_randr_errors_and_the_connection_survives() {
    let connection = connect();
    let randr = connection.randr().expect("RandR");

    let error = connection.crtc_info(BOGUS, 0).expect_err("no such CRTC");
    let Error::Protocol(error) = error else {
        panic!("a refused reply is an X error: {error:?}");
    };
    assert_eq!(
        (error.major_opcode, error.minor_opcode, error.resource_id),
        (randr.major_opcode, GET_CRTC_INFO, BOGUS)
    );
    assert_eq!(randr.error_kind(&error), Some(RandrErrorKind::BadCrtc));

    let error = connection
        .output_info(BOGUS, 0)
        .expect_err("no such output");
    let Error::Protocol(error) = error else {
        panic!("a refused reply is an X error: {error:?}");
    };
    assert_eq!(error.minor_opcode, GET_OUTPUT_INFO);
    assert_eq!(randr.error_kind(&error), Some(RandrErrorKind::BadOutput));

    // A request with no reply: the error comes back through
    // `xcb_request_check`, at the call, rather than as an event later.
    let error = connection.destroy_mode(BOGUS).expect_err("no such mode");
    let Error::Protocol(error) = error else {
        panic!("a checked request's refusal is an X error: {error:?}");
    };
    assert_eq!(error.minor_opcode, DESTROY_MODE);
    assert_eq!(randr.error_kind(&error), Some(RandrErrorKind::BadMode));

    assert_eq!(connection.has_error(), None, "an X error is not fatal");
    assert!(
        drain(&connection)
            .iter()
            .all(|event| !matches!(event, Event::Error(_))),
        "a checked error is not delivered twice"
    );
}

/// Puts the screen back to its connect-time size, and the CRTC switched off
/// to make room back on, if the test that changed them panics — as
/// [`Restore`] does for a mode.
struct RestoreSize<'a> {
    connection: &'a Connection,
    crtc: Crtc,
    before: CrtcInfo,
}

impl RestoreSize<'_> {
    /// Puts both back, as [`restore_size_and_crtc`] does.
    fn restore(&self) -> Result<(), String> {
        restore_size_and_crtc(self.connection, self.crtc, &self.before)
    }
}

impl Drop for RestoreSize<'_> {
    fn drop(&mut self) {
        if std::thread::panicking()
            && let Err(error) = self.restore()
        {
            eprintln!("crcbl e2e: restoring the screen size after a failure: {error}");
        }
    }
}

#[test]
#[ignore = "needs an X server with RandR; run-x11-e2e.sh provides one"]
fn geometry_reads_the_current_screen_size_where_screen_keeps_the_setup_copy() {
    let connection = connect();
    let screen = connection.screen();
    let root = screen.root;

    let before = connection.geometry(root).expect("GetGeometry on the root");
    assert_eq!(
        (before.root, before.x, before.y, before.border_width),
        (root, 0, 0, 0)
    );
    assert_eq!(
        (before.width, before.height),
        (screen.width, screen.height),
        "unchanged, the server and the setup agree"
    );

    // Xvfb's largest screen is the one it started with, so the new size is
    // smaller — and a lit CRTC must fit inside the screen (`BadMatch`
    // otherwise), so it is switched off for the shrink and back on after.
    let range = connection
        .screen_size_range(root)
        .expect("GetScreenSizeRange");
    let shrunk = |now: u16, min: u16| now.saturating_sub(64).max(min);
    let (width, height) = (
        shrunk(screen.width, range.min_width),
        shrunk(screen.height, range.min_height),
    );
    assert_ne!(
        (width, height),
        (screen.width, screen.height),
        "the server's smallest screen ({range:?}) leaves no room to shrink"
    );
    let resources = connection
        .screen_resources_current(root)
        .expect("the resources");
    let (crtc, before_crtc) = lit_crtc(&connection, &resources);
    let restore = RestoreSize {
        connection: &connection,
        crtc,
        before: before_crtc,
    };
    let off = connection
        .set_crtc_config(
            crtc,
            0,
            resources.config_timestamp,
            &CrtcConfig {
                x: 0,
                y: 0,
                mode: 0,
                rotation: Rotation::ROTATE_0,
                outputs: &[],
            },
        )
        .expect("SetCrtcConfig switching the CRTC off");
    assert_eq!(off.status, SetConfigStatus::Success);
    connection
        .set_screen_size(
            root,
            width,
            height,
            scaled_mm(width, screen.width, screen.width_mm),
            scaled_mm(height, screen.height, screen.height_mm),
        )
        .expect("SetScreenSize to a smaller screen");
    connection.sync().expect("the round trip");

    let after = connection.geometry(root).expect("GetGeometry on the root");
    assert_eq!(
        (after.width, after.height),
        (width, height),
        "GetGeometry reports the new size"
    );
    assert_eq!(
        connection.screen(),
        screen,
        "screen() is still the connect-time copy"
    );

    restore
        .restore()
        .expect("the screen size and the CRTC go back");
    let restored = connection.geometry(root).expect("GetGeometry on the root");
    assert_eq!(
        (restored.width, restored.height),
        (screen.width, screen.height)
    );
    let crtc_after = connection
        .crtc_info(crtc, 0)
        .expect("the CRTC after restoring");
    assert_eq!(
        (
            crtc_after.mode,
            crtc_after.x,
            crtc_after.y,
            crtc_after.width,
            crtc_after.height
        ),
        (
            restore.before.mode,
            restore.before.x,
            restore.before.y,
            restore.before.width,
            restore.before.height
        ),
        "the CRTC is lit again where it was"
    );

    let error = connection
        .geometry(0x1)
        .expect_err("0x1 is no client's drawable");
    let Error::Protocol(error) = error else {
        panic!("a refused reply is an X error: {error:?}");
    };
    assert_eq!(
        (
            error.error_code,
            error.major_opcode,
            error.minor_opcode,
            error.resource_id
        ),
        (BAD_DRAWABLE, GET_GEOMETRY, 0, 0x1)
    );
    assert_eq!(connection.has_error(), None, "an X error is not fatal");
}

#[test]
#[ignore = "needs an X server with RandR; run-x11-e2e.sh provides one"]
fn a_grabbed_server_still_answers_its_grabber() {
    let connection = connect();
    connection.grab_server().expect("GrabServer");
    let answered = connection.screen_resources_current(connection.screen().root);
    connection.ungrab_server().expect("UngrabServer");
    answered.expect("the grabbing client is served during its grab");
    connection.sync().expect("the round trip after the grab");
}

#[test]
#[ignore = "needs an X server with RandR; run-x11-e2e.sh provides one"]
fn the_probe_prints_every_output_and_crtc() {
    let connection = connect();
    let resources = connection
        .screen_resources_current(connection.screen().root)
        .expect("the resources");
    let run = crcbl_spawn::command(env!("CARGO_BIN_EXE_crcbl-x11-probe"))
        .output()
        .expect("the probe runs");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&run.stderr)
    );
    for &output in &resources.outputs {
        let name = connection
            .output_info(output, resources.config_timestamp)
            .expect("the output")
            .name;
        assert!(
            stdout.contains(&format!("output {name} ({output:#x})")),
            "{stdout}"
        );
    }
    for &crtc in &resources.crtcs {
        assert!(stdout.contains(&format!("crtc {crtc:#x}:")), "{stdout}");
    }
}
