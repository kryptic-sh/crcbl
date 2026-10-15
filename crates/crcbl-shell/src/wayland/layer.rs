//! Panels as `wlr-layer-shell` surfaces.
//!
//! A [`WindowRole::Panel`](crate::WindowRole::Panel) on this backend is a
//! `zwlr_layer_surface_v1`, which is what [`PanelDesc`] was modelled on, so the
//! translation is field for field: the layer, the anchor, the exclusive zone,
//! the margins and the keyboard interactivity are each one request, all sent
//! before the initial commit because layer-surface state is double-buffered and
//! the first configure answers that commit.
//!
//! What the compositor decides is the size along an axis anchored at both
//! ends. [`layer_size`] sends zero there, which is the protocol's "you assign
//! it", and the configure carries the answer; see
//! [`WaylandShell::layer_configured`].

use crate::{LogicalSize, PanelAnchor, PanelDesc, PanelKeyboard, PanelLayer, Reserve, ShellError};

use super::ffi::{self, WlProxy};
use super::protocol::layer_shell::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use super::{WaylandShell, round_logical};

/// `zwlr_layer_shell_v1` 4 is the lowest version with
/// [`PanelKeyboard::OnDemand`]'s `on_demand` interactivity. 5 adds only
/// `set_exclusive_edge`, which disambiguates a zone on a corner-anchored
/// surface, and [`Reserve::OwnExtent`] refuses corners before this backend
/// sees them.
pub(super) const LAYER_SHELL_VERSION: u32 = 4;

/// The first `zwlr_layer_shell_v1` version with `on_demand` keyboard
/// interactivity.
const ON_DEMAND_SINCE: u32 = 4;

/// The layer-shell `layer` enum for a [`PanelLayer`].
const fn layer_of(layer: PanelLayer) -> u32 {
    match layer {
        PanelLayer::Background => zwlr_layer_shell_v1::layer::BACKGROUND,
        PanelLayer::Bottom => zwlr_layer_shell_v1::layer::BOTTOM,
        PanelLayer::Top => zwlr_layer_shell_v1::layer::TOP,
        PanelLayer::Overlay => zwlr_layer_shell_v1::layer::OVERLAY,
    }
}

/// The layer-shell `anchor` bitfield for a [`PanelAnchor`].
fn anchor_of(anchor: PanelAnchor) -> u32 {
    [
        (PanelAnchor::TOP, zwlr_layer_surface_v1::anchor::TOP),
        (PanelAnchor::BOTTOM, zwlr_layer_surface_v1::anchor::BOTTOM),
        (PanelAnchor::LEFT, zwlr_layer_surface_v1::anchor::LEFT),
        (PanelAnchor::RIGHT, zwlr_layer_surface_v1::anchor::RIGHT),
    ]
    .into_iter()
    .filter(|(edge, _)| anchor.contains(*edge))
    .fold(0, |bits, (_, bit)| bits | bit)
}

/// The layer-shell `keyboard_interactivity` enum for a [`PanelKeyboard`].
const fn keyboard_of(keyboard: PanelKeyboard) -> u32 {
    match keyboard {
        PanelKeyboard::None => zwlr_layer_surface_v1::keyboard_interactivity::NONE,
        PanelKeyboard::OnDemand => zwlr_layer_surface_v1::keyboard_interactivity::ON_DEMAND,
        PanelKeyboard::Exclusive => zwlr_layer_surface_v1::keyboard_interactivity::EXCLUSIVE,
    }
}

/// The `set_size` arguments: the requested size, with zero along every axis
/// the panel is anchored to at both ends.
///
/// Zero is the protocol's "the compositor assigns it", and it is also the
/// only legal value there that [`PanelDesc`] promises: a stretched axis takes
/// its size from the monitor, not from [`WindowDesc::size`](crate::WindowDesc).
/// The other way round is a protocol error — zero along an axis that is *not*
/// anchored at both ends — so a requested zero there is sent as one unit.
fn layer_size(anchor: PanelAnchor, size: LogicalSize) -> (u32, u32) {
    let axis = |ends: PanelAnchor, value: f64| {
        if anchor.contains(ends) {
            0
        } else {
            round_logical(value).max(1).unsigned_abs()
        }
    };
    (
        axis(PanelAnchor::LEFT | PanelAnchor::RIGHT, size.width),
        axis(PanelAnchor::TOP | PanelAnchor::BOTTOM, size.height),
    )
}

/// The exclusive zone for a [`Reserve`].
///
/// [`Reserve::OwnExtent`] is the panel's extent across the edge it reserves.
/// Its margin on that edge is not added: wlroots compositors add the margin to
/// the zone themselves, which is what the protocol's "the exclusive zone
/// includes the margin" means in practice.
fn exclusive_zone(desc: &PanelDesc<'_>, size: LogicalSize) -> i32 {
    match desc.reserve {
        Reserve::None => 0,
        Reserve::IgnoreOthers => -1,
        Reserve::OwnExtent => match desc.reserved_edge() {
            Some(PanelAnchor::LEFT | PanelAnchor::RIGHT) => round_logical(size.width),
            Some(_) => round_logical(size.height),
            // `role::check` refuses `OwnExtent` with no edge before any
            // backend runs, so this is a descriptor that never arrives here.
            None => 0,
        },
    }
}

impl WaylandShell {
    /// Makes `surface` a layer surface and sends its whole initial state.
    ///
    /// The caller commits afterwards, which is what asks for the first
    /// configure.
    ///
    /// # Errors
    ///
    /// [`ShellError::WindowCreation`] if the compositor has withdrawn
    /// `zwlr_layer_shell_v1` or the request fails, and
    /// [`ShellError::Unsupported`] for [`PanelKeyboard::OnDemand`] on a
    /// compositor older than [`ON_DEMAND_SINCE`] — the other two
    /// interactivities would each be a different panel from the one asked for,
    /// one that cannot be typed into or one that takes the keyboard from
    /// everything else.
    pub(super) fn create_layer_surface(
        &mut self,
        surface: *mut WlProxy,
        desc: &PanelDesc<'_>,
        namespace: &core::ffi::CStr,
        size: LogicalSize,
    ) -> Result<*mut WlProxy, ShellError> {
        let shell = self.conn.layer_shell;
        if shell.is_null() {
            return Err(ShellError::WindowCreation(
                "the compositor withdrew zwlr_layer_shell_v1".to_string(),
            ));
        }
        // SAFETY: `shell` was just checked non-null and is a live global.
        if desc.keyboard == PanelKeyboard::OnDemand
            && unsafe { ffi::proxy_version(shell) } < ON_DEMAND_SINCE
        {
            return Err(Self::unsupported(
                "an on-demand panel keyboard (needs zwlr_layer_shell_v1 version 4)",
            ));
        }
        let output = self.output_proxy_for(desc.monitor);
        let (width, height) = layer_size(desc.anchor, size);
        let margins = desc.margins;
        // SAFETY: the shell global and `surface` are live, `surface` has no
        // role and no buffer yet, and a null output is the protocol's own
        // "the compositor chooses". Every later request goes to the layer
        // surface just checked non-null.
        unsafe {
            let layer = zwlr_layer_shell_v1::get_layer_surface(
                shell,
                surface,
                output,
                layer_of(desc.layer),
                namespace,
            );
            if layer.is_null() {
                return Err(ShellError::WindowCreation(
                    "zwlr_layer_shell_v1.get_layer_surface failed".to_string(),
                ));
            }
            zwlr_layer_surface_v1::set_size(layer, width, height);
            zwlr_layer_surface_v1::set_anchor(layer, anchor_of(desc.anchor));
            zwlr_layer_surface_v1::set_exclusive_zone(layer, exclusive_zone(desc, size));
            zwlr_layer_surface_v1::set_margin(
                layer,
                round_logical(margins.top),
                round_logical(margins.right),
                round_logical(margins.bottom),
                round_logical(margins.left),
            );
            zwlr_layer_surface_v1::set_keyboard_interactivity(layer, keyboard_of(desc.keyboard));
            Ok(layer)
        }
    }

    /// A `zwlr_layer_surface_v1.configure`: the size the compositor assigned,
    /// then the same acknowledge-and-publish as a toplevel's configure.
    ///
    /// A zero along an axis is "the client decides", which the protocol only
    /// sends where the panel asked for a size of its own, so the request
    /// stands there.
    pub(super) fn layer_configured(&mut self, layer: usize, serial: u32, width: u32, height: u32) {
        let Some(id) = self.window_by_proxy(layer, |w| w.layer_surface as usize) else {
            return;
        };
        let Ok(window) = self.window_mut(id) else {
            return;
        };
        let requested = window.requested_size;
        window.pending.size = (width > 0 || height > 0).then(|| {
            let axis = |assigned: u32, requested: f64| {
                if assigned > 0 {
                    f64::from(assigned)
                } else {
                    requested
                }
            };
            LogicalSize::new(axis(width, requested.width), axis(height, requested.height))
        });
        self.apply_configure(id, serial);
    }
}

#[cfg(test)]
mod tests {
    use super::{anchor_of, exclusive_zone, keyboard_of, layer_of, layer_size};
    use crate::{LogicalSize, PanelAnchor, PanelDesc, PanelKeyboard, PanelLayer, Reserve};

    const BAR: LogicalSize = LogicalSize::new(0.0, 30.0);

    #[test]
    fn the_layer_shell_values_are_the_protocols_own() {
        assert_eq!(
            [
                PanelLayer::Background,
                PanelLayer::Bottom,
                PanelLayer::Top,
                PanelLayer::Overlay
            ]
            .map(layer_of),
            [0, 1, 2, 3]
        );
        assert_eq!(anchor_of(PanelAnchor::empty()), 0);
        assert_eq!(anchor_of(PanelAnchor::all()), 0b1111);
        assert_eq!(anchor_of(PanelAnchor::TOP | PanelAnchor::RIGHT), 1 | 8);
        assert_eq!(anchor_of(PanelAnchor::BOTTOM | PanelAnchor::LEFT), 2 | 4);
        assert_eq!(
            [
                PanelKeyboard::None,
                PanelKeyboard::Exclusive,
                PanelKeyboard::OnDemand
            ]
            .map(keyboard_of),
            [0, 1, 2]
        );
    }

    #[test]
    fn a_stretched_axis_is_left_to_the_compositor() {
        let across_the_top = PanelAnchor::TOP | PanelAnchor::LEFT | PanelAnchor::RIGHT;
        assert_eq!(layer_size(across_the_top, BAR), (0, 30));
        let down_the_left = PanelAnchor::LEFT | PanelAnchor::TOP | PanelAnchor::BOTTOM;
        assert_eq!(
            layer_size(down_the_left, LogicalSize::new(48.0, 900.0)),
            (48, 0)
        );
        // Centred and unanchored: the request stands on both axes, and a zero
        // where zero would be a protocol error is sent as one unit instead.
        assert_eq!(
            layer_size(PanelAnchor::empty(), LogicalSize::new(600.4, 0.0)),
            (600, 1)
        );
    }

    #[test]
    fn a_reserved_edge_gives_up_the_panels_extent_across_it() {
        let panel = |anchor, reserve| PanelDesc {
            anchor,
            reserve,
            ..PanelDesc::default()
        };
        let top = PanelAnchor::TOP | PanelAnchor::LEFT | PanelAnchor::RIGHT;
        assert_eq!(exclusive_zone(&panel(top, Reserve::OwnExtent), BAR), 30);
        assert_eq!(exclusive_zone(&panel(top, Reserve::None), BAR), 0);
        assert_eq!(exclusive_zone(&panel(top, Reserve::IgnoreOthers), BAR), -1);
        let dock = LogicalSize::new(64.0, 0.0);
        assert_eq!(
            exclusive_zone(&panel(PanelAnchor::RIGHT, Reserve::OwnExtent), dock),
            64
        );
    }
}
