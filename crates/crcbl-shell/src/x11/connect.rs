//! Startup negotiation: which extensions the server has, whether a window
//! manager is running, and what scale the desktop is at.
//!
//! Everything here runs once, in [`X11Shell::open`], and everything it produces
//! is latched — see [`X11Shell::latch_caps`] for why that is not merely a
//! convenience.

use super::{BASE_DPI, Conn, DEFAULT_SCALE, ffi};

impl Conn {
    /// Negotiates RandR, and selects the events that mean "the monitors
    /// changed".
    ///
    /// Returns the extension's first event number, which is what an incoming
    /// event's `response_type` has to be compared against — extension events
    /// are numbered from a base the server assigns per connection, so a
    /// hard-coded number reads an unrelated core event as a RandR one.
    ///
    /// `None` when libxcb-randr is missing, when the server has no RandR, or
    /// when it is older than 1.2 — which is the version that introduced CRTCs,
    /// and therefore the version below which there is no per-monitor geometry
    /// to enumerate at all. A pre-1.2 server had one screen and that was that,
    /// which is exactly what the fallback in [`monitors`](super::monitors)
    /// reports.
    pub(super) fn setup_randr(&self) -> Option<u8> {
        let randr = self.ext.randr?;
        // SAFETY: the connection is live and `randr.id` is the extension
        // descriptor libxcb caches; the reply it returns is owned by libxcb for
        // the connection's lifetime and must not be freed.
        let query =
            unsafe { (self.lib.get_extension_data)(self.raw(), ffi::extension_ptr(randr.id)) };
        if query.is_null() {
            return None;
        }
        // SAFETY: `query` is libxcb's cached reply, valid while the connection
        // is.
        let (present, first_event) = unsafe { ((*query).present, (*query).first_event) };
        if present == 0 {
            log::debug!("the X server has no RANDR extension");
            return None;
        }

        // SAFETY: the connection is live; a null error pointer discards the
        // error, and a null reply is handled below.
        let reply = unsafe {
            let cookie = (randr.query_version)(self.raw(), 1, 3);
            (randr.query_version_reply)(self.raw(), cookie, core::ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let (major, minor) = unsafe { ((*reply).major_version, (*reply).minor_version) };
        // SAFETY: `reply` came from libxcb's `malloc` and is freed once.
        unsafe { ffi::free_reply(reply) };
        if major < 1 || (major == 1 && minor < 2) {
            log::debug!("RANDR {major}.{minor} predates CRTCs; monitors will not be enumerated");
            return None;
        }

        // SAFETY: the connection and root window are live.
        unsafe {
            (randr.select_input)(
                self.raw(),
                self.root,
                ffi::value::RANDR_SCREEN_CHANGE_MASK
                    | ffi::value::RANDR_CRTC_CHANGE_MASK
                    | ffi::value::RANDR_OUTPUT_CHANGE_MASK,
            );
        }
        Some(first_event)
    }

    /// Negotiates XInput 2 and selects raw motion on the root window.
    ///
    /// Returns the extension's **major opcode**, not an event base — XI2
    /// delivers everything as `GeGeneric`, whose `extension` field is the
    /// opcode. That is the only way to tell an XI2 event from any other
    /// extension's generic event.
    ///
    /// # Why the root window, and why raw
    ///
    /// `XI_RawMotion` is only ever delivered to a selection on the **root**:
    /// raw events are pre-transform and pre-clipping, so they do not belong to
    /// any window and the server refuses to deliver them to one. That has a
    /// consequence worth stating, because it is a real difference from
    /// Wayland's `relative-pointer`, which is scoped to a `wl_pointer` on a
    /// focused surface: **this backend sees raw motion whether or not one of
    /// its windows is focused.** [`super::X11Shell`] therefore attributes raw
    /// motion to the focused window and drops it when none of ours is focused,
    /// rather than trusting the event to be about us.
    ///
    /// `None` when libxcb-xinput is missing or the server is XI1-only, and then
    /// [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION) stays clear
    /// and a camera has to difference absolute positions — which is wrong at
    /// the screen edge, which is why the bit exists.
    pub(super) fn setup_xinput2(&self) -> Option<u8> {
        let xi = self.ext.xi?;
        // SAFETY: as `setup_randr` — libxcb owns the returned reply.
        let query = unsafe { (self.lib.get_extension_data)(self.raw(), ffi::extension_ptr(xi.id)) };
        if query.is_null() {
            return None;
        }
        // SAFETY: `query` is libxcb's cached reply.
        let (present, opcode) = unsafe { ((*query).present, (*query).major_opcode) };
        if present == 0 {
            log::debug!("the X server has no XInputExtension");
            return None;
        }

        // SAFETY: the connection is live.
        let reply = unsafe {
            let cookie = (xi.query_version)(self.raw(), 2, 0);
            (xi.query_version_reply)(self.raw(), cookie, core::ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let major = unsafe { (*reply).major_version };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        if major < 2 {
            log::debug!("the X server offers XInput {major} only; no raw motion");
            return None;
        }

        // `xcb_input_event_mask_t` is a two-field header immediately followed
        // by `mask_len` 32-bit words, laid out contiguously. Building it as a
        // `#[repr(C)]` pair is exactly that layout — a struct with the header
        // and one mask word, since one word covers every event number below 32.
        #[repr(C)]
        struct OneWordMask {
            header: ffi::XiEventMask,
            mask: u32,
        }
        let mask = OneWordMask {
            header: ffi::XiEventMask {
                deviceid: ffi::value::XI_ALL_MASTER_DEVICES,
                mask_len: 1,
            },
            mask: ffi::value::XI_RAW_MOTION_MASK,
        };
        // SAFETY: the connection and root are live, and `mask` is one
        // `xcb_input_event_mask_t` followed by its single declared mask word —
        // the layout `XISelectEvents` reads. It outlives the call, which
        // copies it into libxcb's request buffer.
        unsafe {
            (xi.select_events)(
                self.raw(),
                self.root,
                1,
                core::ptr::from_ref(&mask).cast::<ffi::XiEventMask>(),
            );
        }
        Some(opcode)
    }

    /// Whether an EWMH-compliant window manager is running.
    ///
    /// The protocol's own two-step check, and both steps matter: the root
    /// carries `_NET_SUPPORTING_WM_CHECK` naming a window, and **that window
    /// carries the same property naming itself**. EWMH requires the second
    /// step precisely because the first survives a window manager that crashed
    /// — the root property is not cleaned up, so a one-step check reports a
    /// window manager that is not there, and every size hint is then
    /// advertised as honoured when nothing will honour it.
    ///
    /// A window manager started *after* this returns is deliberately not
    /// noticed; see [`X11Shell::latch_caps`](super::X11Shell::latch_caps).
    pub(super) fn detect_window_manager(&self) -> bool {
        let root_says = self.get_property_words(self.root, self.atoms.net_supporting_wm_check);
        let Some(&candidate) = root_says.first() else {
            return false;
        };
        if candidate == 0 {
            return false;
        }
        let it_says = self.get_property_words(candidate, self.atoms.net_supporting_wm_check);
        let confirmed = it_says.first() == Some(&candidate);
        if !confirmed {
            log::debug!(
                "_NET_SUPPORTING_WM_CHECK on the root names window {candidate}, which does \
                 not confirm it — treating that as a window manager that died"
            );
        }
        confirmed
    }

    /// The desktop's scale factor, from `Xft.dpi`.
    ///
    /// # This is a string in a property, and that is not a shortcut
    ///
    /// X11 has no scale-factor protocol. `RESOURCE_MANAGER` on the root window
    /// is the X resource database — a newline-separated text file of
    /// `Name: value` lines, written by `xrdb`, read by every toolkit — and
    /// `Xft.dpi` in it is what GTK, Qt and SDL all actually use. So this parses
    /// text, and doing anything else would disagree with every other window on
    /// the desktop.
    ///
    /// **RandR's physical millimetres are deliberately not used.** They are
    /// available (see
    /// [`RandrOutputInfoReply::mm_width`](super::ffi::RandrOutputInfoReply))
    /// and computing a DPI from them is the classic mistake: a 15-inch 1080p
    /// laptop panel comes out at 141 DPI and would be scaled to 1.47×, which
    /// almost nobody wants, and many monitors report their millimetres wrong
    /// or not at all. Scale is a *preference*, and `Xft.dpi` is where the user
    /// expressed it.
    ///
    /// Clamped to a sane band: a malformed resource file must not produce a
    /// window a thousand times too big.
    pub(super) fn read_xft_scale(&self) -> f64 {
        let Some((_, _, bytes)) = self.get_property(self.root, ffi::value::ATOM_RESOURCE_MANAGER)
        else {
            return DEFAULT_SCALE;
        };
        let Ok(text) = core::str::from_utf8(&bytes) else {
            return DEFAULT_SCALE;
        };
        parse_xft_dpi(text).map_or(DEFAULT_SCALE, |dpi| (dpi / BASE_DPI).clamp(0.5, 8.0))
    }
}

/// `Xft.dpi` out of an X resource database, or `None`.
///
/// A free function so the parser — the part that can be wrong about a real
/// user's `.Xresources` — is testable with no server. The format is
/// deliberately taken narrowly: `Xft.dpi` at the start of a line, a colon,
/// whitespace, a number. Resource files support wildcards, includes and
/// per-application prefixes, and honouring those would mean implementing
/// `xrdb`; what every toolkit actually looks for is this one line.
fn parse_xft_dpi(database: &str) -> Option<f64> {
    for line in database.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("Xft.dpi") else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix(':') else {
            // `Xft.dpiFoo:` is a different resource, not this one.
            continue;
        };
        if let Ok(dpi) = value.trim().parse::<f64>()
            && dpi.is_finite()
            && dpi > 0.0
        {
            return Some(dpi);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xft_dpi_is_read_the_way_a_real_xresources_file_writes_it() {
        // `xrdb` emits tab-separated lines; users write spaces. Both appear in
        // the wild and both have to work, because the alternative is a window
        // that is the wrong size on someone's desktop and right on ours.
        assert_eq!(parse_xft_dpi("Xft.dpi:\t192\n"), Some(192.0));
        assert_eq!(parse_xft_dpi("Xft.dpi: 120"), Some(120.0));
        assert_eq!(parse_xft_dpi("  Xft.dpi:96  "), Some(96.0));
        assert_eq!(
            parse_xft_dpi("*background:\t#000000\nXft.antialias:\t1\nXft.dpi:\t144\n"),
            Some(144.0),
            "one line among many"
        );
    }

    #[test]
    fn a_resource_that_merely_starts_with_the_name_is_not_the_one() {
        // `Xft.dpiScale` is not `Xft.dpi`, and a prefix match would read a
        // different setting's value as the scale.
        assert_eq!(parse_xft_dpi("Xft.dpiScale:\t2\n"), None);
        assert_eq!(parse_xft_dpi("Xft.hinting:\t1\n"), None);
        assert_eq!(parse_xft_dpi(""), None);
        assert_eq!(parse_xft_dpi("Xft.dpi:\n"), None, "no value");
        assert_eq!(parse_xft_dpi("Xft.dpi:\tnonsense\n"), None);
        assert_eq!(parse_xft_dpi("Xft.dpi:\t0\n"), None, "zero is not a DPI");
        assert_eq!(parse_xft_dpi("Xft.dpi:\t-96\n"), None);
    }

    #[test]
    fn the_scale_is_the_dpi_over_ninety_six() {
        // The conversion every toolkit agrees on. 120 is the "125%" every
        // desktop settings panel offers, and it is *not* an integer scale —
        // which is what `ShellCaps::FRACTIONAL_SCALE` is asserting on X11.
        let scale = |dpi: f64| (dpi / BASE_DPI).clamp(0.5, 8.0);
        assert!((scale(96.0) - 1.0).abs() < f64::EPSILON);
        assert!((scale(120.0) - 1.25).abs() < f64::EPSILON);
        assert!((scale(144.0) - 1.5).abs() < f64::EPSILON);
        assert!((scale(192.0) - 2.0).abs() < f64::EPSILON);
        // A malformed database must not produce a window a thousand times too
        // big, or one that rounds to zero pixels.
        assert!((scale(1e9) - 8.0).abs() < f64::EPSILON);
        assert!((scale(1.0) - 0.5).abs() < f64::EPSILON);
    }
}
