//! Themed cursor shapes, through libxcb-cursor.
//!
//! X11 has no compositor to name a shape to, as Wayland's `cursor-shape-v1`
//! does: the client loads the cursor itself and sets it on the window's
//! `CWCursor`. libxcb-cursor is that loader — it reads `Xcursor.theme` and
//! `Xcursor.size` from the resource database, finds the theme's file for a
//! name, and uploads it as a cursor, or falls back to a glyph from the core
//! cursor font when the name is one of that font's and no theme has it. So a
//! shape is a list of names to try, newest vocabulary first; see
//! [`theme_names`].
//!
//! A loaded cursor is cached per [`CursorIcon`] for the connection's lifetime,
//! because a UI sets the cursor on every hover and each load reads files and
//! uploads an image. A shape none of whose names loaded is cached too, as
//! `XCB_NONE` — inherit the root's cursor — so a theme without it is not
//! searched again on every hover.
//!
//! [`ShellCaps::CURSOR_SHAPE`](crate::ShellCaps::CURSOR_SHAPE) follows
//! [`Cursors::applies_shapes`]: the library loaded *and* its context was
//! created on this connection.

use core::ffi::CStr;
use core::ptr::{self, NonNull};
use std::collections::HashMap;

use super::ffi::{self, Connection, CursorContext, CursorLib, Lib, Screen};
use crate::CursorIcon;

/// The X cursor names to try for `icon`, in order.
///
/// The CSS keyword first — the name freedesktop cursor themes index by, and
/// the one [`CursorIcon::as_css_name`] speaks — then the legacy names older
/// themes ship instead. The last name of every list is a glyph of the core
/// cursor font (`X11/cursorfont.h`), which libxcb-cursor falls back to when
/// no theme has the name, so a shape still resolves on a system with no cursor
/// theme installed.
pub(super) const fn theme_names(icon: CursorIcon) -> &'static [&'static CStr] {
    match icon {
        CursorIcon::Default => &[c"default", c"left_ptr"],
        CursorIcon::Pointer => &[c"pointer", c"hand2"],
        CursorIcon::Text => &[c"text", c"xterm"],
        CursorIcon::Crosshair => &[c"crosshair", c"cross"],
        CursorIcon::Wait => &[c"wait", c"watch"],
        CursorIcon::Progress => &[c"progress", c"left_ptr_watch", c"watch"],
        CursorIcon::NotAllowed => &[c"not-allowed", c"crossed_circle", c"circle"],
        CursorIcon::Grab => &[c"grab", c"openhand", c"hand1"],
        CursorIcon::Grabbing => &[c"grabbing", c"closedhand", c"fleur"],
        CursorIcon::Move => &[c"move", c"fleur"],
        CursorIcon::ResizeNorthSouth => &[c"ns-resize", c"sb_v_double_arrow"],
        CursorIcon::ResizeEastWest => &[c"ew-resize", c"sb_h_double_arrow"],
        CursorIcon::ResizeNorthEastSouthWest => {
            &[c"nesw-resize", c"fd_double_arrow", c"bottom_left_corner"]
        }
        CursorIcon::ResizeNorthWestSouthEast => {
            &[c"nwse-resize", c"bd_double_arrow", c"bottom_right_corner"]
        }
        CursorIcon::ResizeColumn => &[c"col-resize", c"sb_h_double_arrow"],
        CursorIcon::ResizeRow => &[c"row-resize", c"sb_v_double_arrow"],
    }
}

/// The cursor for `icon` from `cache`, or the first of its
/// [`theme_names`] that `load` turns into one, remembered either way.
///
/// `load` answers `XCB_NONE` for a name it cannot find; when every name does,
/// `XCB_NONE` is what is cached and returned.
fn resolve(
    cache: &mut HashMap<CursorIcon, u32>,
    icon: CursorIcon,
    mut load: impl FnMut(&CStr) -> u32,
) -> u32 {
    *cache.entry(icon).or_insert_with(|| {
        theme_names(icon)
            .iter()
            .map(|name| load(name))
            .find(|&cursor| cursor != ffi::value::NONE)
            .unwrap_or(ffi::value::NONE)
    })
}

/// libxcb-cursor's context on this connection, when there is one.
struct Theme {
    lib: &'static CursorLib,
    context: NonNull<CursorContext>,
}

/// The themed cursors this connection has loaded, and the means to load more.
pub(super) struct Cursors {
    theme: Option<Theme>,
    /// Every shape asked for so far, and its cursor — `XCB_NONE` for one that
    /// loaded nothing.
    loaded: HashMap<CursorIcon, u32>,
}

impl Cursors {
    /// No theme: every shape answers `XCB_NONE`.
    pub(super) fn unthemed() -> Self {
        Self {
            theme: None,
            loaded: HashMap::new(),
        }
    }

    /// Creates libxcb-cursor's context on `connection`, if the library loaded.
    ///
    /// A round trip or two — the context reads the resource database and asks
    /// whether the server has RENDER — which is why it is made once, at
    /// connect time, rather than on the first shape.
    ///
    /// # Safety
    ///
    /// `connection` must be live and error-free, and `screen` must be the
    /// screen from that connection's setup reply, which libxcb owns for the
    /// connection's lifetime.
    pub(super) unsafe fn open(
        lib: Option<&'static CursorLib>,
        connection: *mut Connection,
        screen: *mut Screen,
    ) -> Self {
        let Some(lib) = lib else {
            return Self::unthemed();
        };
        let mut context = ptr::null_mut();
        // SAFETY: the caller guarantees the connection and screen; `context`
        // is a live local the call writes the new context's address into.
        let status = unsafe { (lib.context_new)(connection, screen, &raw mut context) };
        // libxcb-cursor fails only when it cannot allocate the context, and
        // then leaves the out-pointer null, so a failure has nothing to free.
        let theme = match NonNull::new(context) {
            Some(context) if status >= 0 => Some(Theme { lib, context }),
            _ => {
                crcbl_core::log::warn!(
                    "xcb_cursor_context_new failed ({status}): cursor shapes are recorded and \
                     not applied, so ShellCaps::CURSOR_SHAPE stays clear"
                );
                None
            }
        };
        Self {
            theme,
            loaded: HashMap::new(),
        }
    }

    /// Whether a shape is applied — the
    /// [`CURSOR_SHAPE`](crate::ShellCaps::CURSOR_SHAPE) bit.
    pub(super) const fn applies_shapes(&self) -> bool {
        self.theme.is_some()
    }

    /// The cursor to set for `icon`: loaded on first use and cached, or
    /// `XCB_NONE` — inherit the root's — when there is no theme or none of the
    /// shape's names loaded.
    pub(super) fn cursor(&mut self, icon: CursorIcon) -> u32 {
        let Some(theme) = &self.theme else {
            return ffi::value::NONE;
        };
        let (load, context) = (theme.lib.load_cursor, theme.context);
        resolve(&mut self.loaded, icon, |name| {
            // SAFETY: the context is live until `release`, which also takes
            // the theme away, and `name` is NUL-terminated and outlives the
            // call.
            unsafe { load(context.as_ptr(), name.as_ptr()) }
        })
    }

    /// Frees every loaded cursor and the context, before the connection
    /// closes.
    ///
    /// # Safety
    ///
    /// `connection` must be the connection this was opened on, still open.
    pub(super) unsafe fn release(&mut self, lib: &Lib, connection: *mut Connection) {
        for (_, cursor) in self.loaded.drain() {
            if cursor != ffi::value::NONE {
                // SAFETY: the caller guarantees the connection, and `cursor`
                // was created on it by libxcb-cursor and is freed once, here,
                // as the drain empties the cache.
                unsafe { (lib.free_cursor)(connection, cursor) };
            }
        }
        if let Some(theme) = self.theme.take() {
            // SAFETY: the context was created on this still-open connection by
            // `open` and is freed once, here, as `take` removes it.
            unsafe { (theme.lib.context_free)(theme.context.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph name in `X11/cursorfont.h`, the core cursor font every X
    /// server has.
    const CORE_CURSOR_FONT: &[&str] = &[
        "X_cursor",
        "arrow",
        "based_arrow_down",
        "based_arrow_up",
        "boat",
        "bogosity",
        "bottom_left_corner",
        "bottom_right_corner",
        "bottom_side",
        "bottom_tee",
        "box_spiral",
        "center_ptr",
        "circle",
        "clock",
        "coffee_mug",
        "cross",
        "cross_reverse",
        "crosshair",
        "diamond_cross",
        "dot",
        "dotbox",
        "double_arrow",
        "draft_large",
        "draft_small",
        "draped_box",
        "exchange",
        "fleur",
        "gobbler",
        "gumby",
        "hand1",
        "hand2",
        "heart",
        "icon",
        "iron_cross",
        "left_ptr",
        "left_side",
        "left_tee",
        "leftbutton",
        "ll_angle",
        "lr_angle",
        "man",
        "middlebutton",
        "mouse",
        "pencil",
        "pirate",
        "plus",
        "question_arrow",
        "right_ptr",
        "right_side",
        "right_tee",
        "rightbutton",
        "rtl_logo",
        "sailboat",
        "sb_down_arrow",
        "sb_h_double_arrow",
        "sb_left_arrow",
        "sb_right_arrow",
        "sb_up_arrow",
        "sb_v_double_arrow",
        "shuttle",
        "sizing",
        "spider",
        "spraycan",
        "star",
        "target",
        "tcross",
        "top_left_arrow",
        "top_left_corner",
        "top_right_corner",
        "top_side",
        "top_tee",
        "trek",
        "ul_angle",
        "umbrella",
        "ur_angle",
        "watch",
        "xterm",
    ];

    fn names(icon: CursorIcon) -> Vec<&'static str> {
        theme_names(icon)
            .iter()
            .map(|name| name.to_str().expect("an ASCII name"))
            .collect()
    }

    #[test]
    fn every_icon_tries_its_css_keyword_first_and_ends_on_a_core_font_glyph() {
        for &icon in CursorIcon::ALL {
            let names = names(icon);
            assert!(names.len() >= 2, "{icon:?} has a fallback: {names:?}");
            assert_eq!(names[0], icon.as_css_name(), "{icon:?} starts at CSS");
            let last = names[names.len() - 1];
            assert!(
                CORE_CURSOR_FONT.contains(&last),
                "{icon:?} ends on `{last}`, which the core cursor font lacks"
            );
        }
    }

    #[test]
    fn the_legacy_names_are_the_ones_older_themes_ship() {
        assert_eq!(names(CursorIcon::Default), ["default", "left_ptr"]);
        assert_eq!(names(CursorIcon::Text), ["text", "xterm"]);
        assert_eq!(names(CursorIcon::Pointer), ["pointer", "hand2"]);
        assert_eq!(names(CursorIcon::Wait), ["wait", "watch"]);
        assert_eq!(
            names(CursorIcon::ResizeEastWest),
            ["ew-resize", "sb_h_double_arrow"]
        );
    }

    #[test]
    fn names_are_tried_in_order_until_one_loads() {
        let mut cache = HashMap::new();
        let mut tried = Vec::new();
        let cursor = resolve(&mut cache, CursorIcon::Text, |name| {
            tried.push(name.to_owned());
            if name == c"xterm" {
                7
            } else {
                ffi::value::NONE
            }
        });
        assert_eq!(cursor, 7);
        assert_eq!(tried, [c"text".to_owned(), c"xterm".to_owned()]);

        // The first name that loads ends the search.
        let mut tried = 0;
        let cursor = resolve(&mut cache, CursorIcon::Pointer, |_| {
            tried += 1;
            9
        });
        assert_eq!((cursor, tried), (9, 1));
    }

    #[test]
    fn a_shape_is_loaded_once_including_one_that_loaded_nothing() {
        let mut cache = HashMap::new();
        let mut loads = 0;
        for _ in 0..3 {
            let cursor = resolve(&mut cache, CursorIcon::Default, |_| {
                loads += 1;
                5
            });
            assert_eq!(cursor, 5);
        }
        assert_eq!(loads, 1, "a repeat hover reuses the loaded cursor");

        let mut loads = 0;
        for _ in 0..3 {
            let cursor = resolve(&mut cache, CursorIcon::Grab, |_| {
                loads += 1;
                ffi::value::NONE
            });
            assert_eq!(cursor, ffi::value::NONE);
        }
        assert_eq!(
            loads,
            theme_names(CursorIcon::Grab).len(),
            "every name is tried once, and the miss is remembered"
        );
    }

    #[test]
    fn without_a_theme_no_shape_is_applied() {
        let mut cursors = Cursors::unthemed();
        assert!(!cursors.applies_shapes(), "CURSOR_SHAPE stays clear");
        for &icon in CursorIcon::ALL {
            assert_eq!(cursors.cursor(icon), ffi::value::NONE);
        }
    }
}
