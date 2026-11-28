//! Panels as `wlr-layer-shell` surfaces.
//!
//! A [`WindowRole::Panel`](crate::WindowRole::Panel) on this backend is a
//! `zwlr_layer_surface_v1`, which is what [`PanelDesc`] was modelled on, so the
//! translation is field for field: the layer, the anchor, the exclusive zone,
//! the margins and the keyboard interactivity are each one request, all sent
//! before the initial commit because layer-surface state is double-buffered and
//! the first configure answers that commit.
//!
//! [`Shell::set_panel`] sends the same requests again, `set_layer` with them,
//! and commits: the compositor answers with a configure, which reports the
//! new size as the first one did. A panel on screen given a new size of its
//! own is the exception: the caller is told the size at once, and the
//! requests go out with the frame drawn at it; see
//! [`WaylandShell::reconfigure_layer_surface`]. The namespace and the output
//! are arguments of `get_layer_surface` and nothing changes them afterwards.
//!
//! What the compositor decides is the size along an axis anchored at both
//! ends. [`layer_size`] sends zero there, which is the protocol's "you assign
//! it", and the configure carries the answer; see
//! [`WaylandShell::layer_configured`].

use std::sync::Arc;

use crate::role::KeptPanel;
use crate::{
    DisplayMode, LogicalSize, PanelAnchor, PanelDesc, PanelKeyboard, PanelLayer, Reserve, Shell,
    ShellError, ShellEvent, WindowConfiguration, WindowId,
};

use super::ffi::{self, WlProxy};
use super::owed::{Geometry, OwedPanel, PanelReply, WindowFacts};
use super::present::{self, Region};
use super::protocol::layer_shell::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use super::protocol::wayland::{wl_region, wl_surface};
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

/// The first `zwlr_layer_surface_v1` version with `set_layer`; before it a
/// layer surface stays in the layer it was made in.
const SET_LAYER_SINCE: u32 = 2;

/// The layer-shell `layer` enum for a [`PanelLayer`].
pub(super) const fn layer_of(layer: PanelLayer) -> u32 {
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

/// Refuses [`PanelKeyboard::OnDemand`] on a layer shell of `version` older
/// than [`ON_DEMAND_SINCE`]: the other two interactivities would each be a
/// different panel from the one asked for, one that cannot be typed into or
/// one that takes the keyboard from everything else.
fn check_keyboard(keyboard: PanelKeyboard, version: u32) -> Result<(), ShellError> {
    if keyboard == PanelKeyboard::OnDemand && version < ON_DEMAND_SINCE {
        return Err(WaylandShell::unsupported(
            "an on-demand panel keyboard (needs zwlr_layer_shell_v1 version 4)",
        ));
    }
    Ok(())
}

/// Sends a layer surface every piece of its state but the layer, which
/// `get_layer_surface` takes and `set_layer` changes.
///
/// # Safety
///
/// `layer` must be a live `zwlr_layer_surface_v1`.
pub(super) unsafe fn send_state(layer: *mut WlProxy, desc: &PanelDesc<'_>, size: LogicalSize) {
    let (width, height) = layer_size(desc.anchor, size);
    let margins = desc.margins;
    // SAFETY: the caller guarantees a live layer surface, and each request
    // takes plain integers.
    unsafe {
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
    /// compositor older than [`ON_DEMAND_SINCE`]; see [`check_keyboard`].
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
        check_keyboard(desc.keyboard, unsafe { ffi::proxy_version(shell) })?;
        let output = self.output_proxy_for(desc.monitor);
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
            send_state(layer, desc, size);
            Ok(layer)
        }
    }

    /// Sends a panel's layer surface its new state and commits it, for
    /// [`Shell::set_panel`]. The compositor answers with a configure, which
    /// [`layer_configured`](Self::layer_configured) takes as it took the first.
    ///
    /// **A panel on screen given a new size of its own is not sent it here,**
    /// when it presents through its hook. Its buffer is the old size, and
    /// Hyprland 0.56.2 stretches a layer surface's buffer over its new
    /// rectangle from the commit that changes it, so a menu shrinking from a
    /// full frame to a short one showed every old row squeezed for a frame.
    /// The size is the client's own to choose, so the caller is told it at
    /// once, as a [`ShellEvent::Resized`], and the request is owed to the
    /// frame drawn at it (see [`owed`](super::owed)): a frame still at the old
    /// size carries nothing of it. A later request replaces it, and one at the
    /// size on screen with nothing else waiting — a margin moved, say — is
    /// committed at once.
    ///
    /// Committed at once as before: a size along a stretched axis, which is
    /// the compositor's to assign and so withdraws a request waiting; a panel
    /// not on screen, with no buffer to stretch; and a panel whose presents do
    /// not come through its hook, which is told its size by the configure.
    ///
    /// # Errors
    ///
    /// What [`crate::role::check_set_panel`] refuses, and
    /// [`ShellError::Unsupported`] for [`PanelKeyboard::OnDemand`] on a layer
    /// shell older than [`ON_DEMAND_SINCE`] or a new layer on one older than
    /// [`SET_LAYER_SINCE`].
    pub(super) fn reconfigure_layer_surface(
        &mut self,
        window: WindowId,
        size: LogicalSize,
        desc: &PanelDesc<'_>,
    ) -> Result<(), ShellError> {
        let state = self.window(window)?;
        let was = state.panel.as_ref().map(crate::role::KeptPanel::desc);
        crate::role::check_set_panel(window, was, desc, self.caps, self.backend(), |monitor| {
            self.monitor(monitor).is_some()
        })?;
        let (layer, surface) = (state.layer_surface, state.surface);
        let was_layer = was.map(|was| was.layer);
        if layer.is_null() {
            return Err(ShellError::Backend(format!(
                "set_panel: {window:?} has no layer surface"
            )));
        }
        // SAFETY: a live layer surface, checked non-null just above; a layer
        // surface has its shell's version.
        let version = unsafe { ffi::proxy_version(layer) };
        check_keyboard(desc.keyboard, version)?;
        let set_layer = version >= SET_LAYER_SINCE;
        if !set_layer && was_layer != Some(desc.layer) {
            return Err(Self::unsupported(
                "changing a panel's layer (needs zwlr_layer_shell_v1 version 2)",
            ));
        }

        let (width, height) = layer_size(desc.anchor, size);
        let scale_factor = state.scale_factor();
        let logical = LogicalSize::new(f64::from(width), f64::from(height));
        let physical = state
            .requested_constraints
            .apply(logical.to_physical(scale_factor), scale_factor);
        let present = Arc::clone(&state.present);
        let proxies = state.proxies();
        let mut shared = present.lock();
        // A given-up window cannot wait, so this request is committed at once;
        // what it still owed goes ahead of it, never after.
        let settled = shared.settle_given_up();
        let presenting = shared.presenting();
        let reply = shared.owed.set_panel(
            OwedPanel {
                size: logical,
                desc: KeptPanel::new(desc),
                set_layer,
            },
            Geometry {
                size: physical,
                logical,
            },
            width > 0 && height > 0,
            WindowFacts {
                presenting,
                on_screen: !state.outputs.is_empty(),
                ..WindowFacts::default()
            },
        );
        let (at_once, region) = match reply {
            PanelReply::Owed => (false, None),
            PanelReply::AtOnce { input_region } => (true, input_region),
        };
        drop(shared);

        if at_once {
            // SAFETY: the window's own live layer surface and `wl_surface`; the
            // region is null or one `set_input_region` made, destroyed once
            // set; the commit applies the double-buffered state just sent.
            unsafe {
                present::send_settled(&proxies, settled);
                send_state(layer, desc, size);
                if set_layer {
                    zwlr_layer_surface_v1::set_layer(layer, layer_of(desc.layer));
                }
                if let Some(Region(region)) = region {
                    wl_surface::set_input_region(surface, region);
                    if !region.is_null() {
                        wl_region::destroy(region);
                    }
                }
                wl_surface::commit(surface);
            }
            self.conn.flush();
        }
        let state = self.window_mut(window)?;
        state.panel = Some(KeptPanel::new(desc));
        state.requested_size = size;
        if !at_once {
            // The size the caller draws at from here, which the frame that
            // lands the request is drawn at.
            state.pending.size = Some(logical);
            let config = WindowConfiguration {
                size: physical,
                scale_factor,
                mode: DisplayMode::Windowed,
            };
            if state.configuration != Some(config) {
                state.configuration = Some(config);
                self.queue.push_back(ShellEvent::Resized {
                    window,
                    size: physical,
                    scale_factor,
                });
            }
        }
        Ok(())
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
