//! What a window owes the frame drawn at its new size.
//!
//! Every piece of surface state is applied by the next `wl_surface.commit`,
//! whatever that commit carries. A new size sent ahead of a frame drawn at the
//! old one is applied to the old buffer: the compositor shows the last frame
//! stretched into the new rectangle, which is how lyr's shrinking menu showed
//! its old rows squeezed on Hyprland. Holding back only the commit is not
//! enough, because the next commit may well be the swapchain's present of a
//! frame drawn before the caller heard of the new size.
//!
//! So state that belongs with a size is not sent when it is produced. It is
//! recorded here, as data, and sent by the window's
//! [`PresentHook`](crate::PresentHook) just before a present —
//! [`Owed::take_for_frame`] decides what that frame carries — and the
//! present's own commit applies it with the frame:
//!
//! - a panel's own new size, from [`Shell::set_panel`](crate::Shell::set_panel);
//! - the acknowledgement of a configure that resizes the window, with its
//!   viewport destination;
//! - a new integer buffer scale, and the destination after a rescale;
//! - an input region set while a new size waits, which is the new layout's.
//!
//! **Which frame carries it.** The frame drawn at the size waited for, the
//! target, carries everything. A frame at the size of an older configure
//! still waiting — a resize storm, where the caller lags behind the
//! compositor — acknowledges that one, and the newer ones keep waiting. A
//! frame at another size presented before the caller could have heard of the
//! target carries nothing, so the geometry on screen stays the one it was
//! drawn for. Once the shell's pump has handed out the `Resized` or
//! `ScaleFactorChanged` telling the target ([`Owed::told`]), the next frame
//! carries everything, whatever its size. A caller that redrew at the new
//! size gets a clean frame. One that ignores `Resized` gets its old frame
//! scaled into the new geometry, which is what every frame of it got before
//! frames carried their own geometry; only now it is one pump later, after the
//! caller had its chance. Nothing waits for ever.
//!
//! That rule costs the engine nothing: its loop resizes in the pump at the
//! top of the frame it presents, so its first frame after the pump that
//! told it is already at the new size, and matches the target.
//!
//! **An integer buffer scale goes only with a buffer whose size it divides.**
//! Committing one onto any other is `invalid_size`, which ends the
//! connection. One held back from a frame waits for the next frame it
//! divides, without holding anything else up.
//!
//! **A present that fails puts its frame back.** What it sent is on the wire
//! with no present to commit it, so the next commit of any kind would apply
//! it to the old buffer: [`present_failed`](Owed::present_failed) makes its
//! target the target again, so everything the shell would commit at once
//! waits for a frame instead, and hands back the buffer scale in effect to
//! send over a new one the failed frame sent.
//!
//! **Whenever an acknowledgement goes out, no older configure stays owed.**
//! Acknowledging a serial older than one already acknowledged is a protocol
//! error (`invalid_serial` on wlroots), so every path that acknowledges at
//! once — a configure that fits the buffer on screen with nothing waiting,
//! one for a window off screen or not presenting through its hook — takes
//! the older ones with it.
//!
//! Only a window that presents through its hook has anything owed. Until its
//! first [`take_for_frame`](Owed::take_for_frame) — a window whose presents
//! never come through crcbl, or the null device — and again after
//! [`give_up`](Owed::give_up), [`presenting`](Owed::presenting) is `false`
//! and the shell sends everything as it comes.
//!
//! Pure: no proxies, so every rule is tested here without a compositor. `R`
//! is the region handle, a `wl_region` proxy in the backend.

use crate::role::KeptPanel;
use crate::{LogicalSize, PhysicalSize};

/// Configures kept waiting for a frame at their size. Acknowledging one
/// acknowledges every older one too, so the oldest beyond this are dropped
/// rather than kept: a caller that never catches up acknowledges with the
/// frame at the newest.
const MAX_OWED_CONFIGURES: usize = 8;

/// The buffer scale a surface has before anything sets one.
const DEFAULT_BUFFER_SCALE: i32 = 1;

/// A size the window was told it has, as the frame drawn at it has it: the
/// buffer in device pixels, and the viewport destination in logical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Geometry {
    pub(super) size: PhysicalSize,
    pub(super) logical: LogicalSize,
}

/// A [`Shell::set_panel`](crate::Shell::set_panel) waiting for the frame at
/// its size: everything `send_state` sends, and whether `set_layer` goes
/// with it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct OwedPanel {
    pub(super) size: LogicalSize,
    pub(super) desc: KeptPanel,
    pub(super) set_layer: bool,
}

/// What a frame carries: each field, when set, is one request to send ahead
/// of the present's commit.
#[derive(Debug, PartialEq)]
pub(super) struct Batch<R> {
    pub(super) panel: Option<OwedPanel>,
    pub(super) ack: Option<u32>,
    pub(super) buffer_scale: Option<i32>,
    pub(super) viewport: Option<LogicalSize>,
    /// `Some` with the region to set, which is itself the protocol's "null
    /// for the whole surface" when the caller asked for that.
    pub(super) input_region: Option<R>,
}

impl<R> Batch<R> {
    const fn empty() -> Self {
        Self {
            panel: None,
            ack: None,
            buffer_scale: None,
            viewport: None,
            input_region: None,
        }
    }

    #[cfg(test)]
    const fn is_empty(&self) -> bool {
        self.panel.is_none()
            && self.ack.is_none()
            && self.buffer_scale.is_none()
            && self.viewport.is_none()
            && self.input_region.is_none()
    }
}

/// What the shell knows of a window as a configure, a rescale or a panel
/// request arrives, beyond what is owed: what [`Owed::configure`],
/// [`Owed::rescale`] and [`Owed::set_panel`] decide on.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct WindowFacts {
    /// Whether its frames come through its hook, so state may wait for one.
    pub(super) presenting: bool,
    /// Whether it is on an output, with a buffer on screen to distort.
    pub(super) on_screen: bool,
    /// Whether the scale changed from the configuration before.
    pub(super) scale_changed: bool,
    /// Whether the size or scale changed from the configuration before.
    pub(super) resized: bool,
}

/// What a configure or a rescale sends now: the batch, and whether the
/// shell commits it at once rather than leaving that to the caller's next
/// commit.
#[derive(Debug, PartialEq)]
pub(super) struct Now<R> {
    pub(super) batch: Batch<R>,
    pub(super) commit: bool,
}

/// What [`Owed::set_panel`] decided.
#[derive(Debug, PartialEq)]
pub(super) enum PanelReply<R> {
    /// Owed to the frame at its size: the caller is told it at once.
    Owed,
    /// Sent and committed at once, with the input region that waited for a
    /// request it withdrew, if nothing else waits now.
    AtOnce { input_region: Option<R> },
}

/// The state a frame's batch replaced, to put back if its present fails:
/// recorded for every frame that sends a target's state or a buffer scale,
/// which are what a commit of the old buffer could not take. An
/// acknowledgement is not among it: once out, it is never sent again.
#[derive(Debug)]
struct InFlight {
    /// The target the frame took, if it was due.
    target: Option<Geometry>,
    /// The buffer scale it sent, and the one in effect before it.
    scale: Option<(i32, Option<i32>)>,
}

/// One window's owed state; see the [module docs](self).
#[derive(Debug)]
pub(super) struct Owed<R> {
    /// The size of the last frame presented through the hook, which is the
    /// buffer on screen; `None` before the first, and once given up.
    shown: Option<PhysicalSize>,
    /// The size shown before the last frame, put back if its present fails:
    /// the buffer up is still that one.
    before: Option<PhysicalSize>,
    /// What the last frame's batch replaced; see [`InFlight`].
    in_flight: Option<InFlight>,
    /// The buffer scale last sent, by a batch or by the shell itself; `None`
    /// before any, which is [`DEFAULT_BUFFER_SCALE`].
    scale: Option<i32>,
    /// The size last handed to the caller in a `Resized` or
    /// `ScaleFactorChanged`; see [`told`](Self::told).
    told: Option<PhysicalSize>,
    /// Resizing configures not yet acknowledged, oldest first.
    configures: Vec<(u32, Geometry)>,
    /// The waiting panel request, and the geometry of the frame it waits for.
    panel: Option<(OwedPanel, Geometry)>,
    /// The geometry everything owed waits for: the size the window was last
    /// told, in the `Resized` or `ScaleFactorChanged` that told it.
    target: Option<Geometry>,
    /// The target a rescale set, kept for a panel request it waited behind
    /// and that is withdrawn; see [`abandon_panel`](Self::abandon_panel).
    rescaled: Option<Geometry>,
    buffer_scale: Option<i32>,
    input_region: Option<R>,
}

impl<R> Default for Owed<R> {
    fn default() -> Self {
        Self {
            shown: None,
            before: None,
            in_flight: None,
            scale: None,
            told: None,
            configures: Vec::new(),
            panel: None,
            target: None,
            rescaled: None,
            buffer_scale: None,
            input_region: None,
        }
    }
}

/// Whether `scale` may be committed with a buffer of `buffer`'s size: one of
/// no size yet, or one it divides.
fn divides(buffer: Option<PhysicalSize>, scale: i32) -> bool {
    buffer.is_none_or(|buffer| {
        u32::try_from(scale)
            .is_ok_and(|scale| scale > 0 && buffer.width % scale == 0 && buffer.height % scale == 0)
    })
}

impl<R> Owed<R> {
    /// Whether the window presents through its hook, which is what lets state
    /// wait for a frame at all.
    pub(super) const fn presenting(&self) -> bool {
        self.shown.is_some()
    }

    /// Whether anything waits for a frame at a new size, so a commit made
    /// now would come before it.
    pub(super) const fn resizing(&self) -> bool {
        self.target.is_some()
    }

    /// The logical size a configure's own, `configured`, gives way to: a
    /// panel request waiting for its frame is the size the caller was told
    /// and draws at, which the compositor configured against the request
    /// before it, and configures again once the waiting one lands.
    pub(super) fn logical_for(&self, configured: LogicalSize) -> LogicalSize {
        self.panel
            .as_ref()
            .map_or(configured, |(panel, _)| panel.size)
    }

    /// The buffer scale the shell set itself, outside any batch: the one in
    /// effect, to put back over one a failed frame sent.
    pub(super) fn scale_sent(&mut self, scale: i32) {
        self.scale = Some(scale);
    }

    /// What a configure `serial` at `geometry` does: owed to the frame at its
    /// size (`None`), or acknowledged now with everything older.
    ///
    /// - Presenting and on screen: owed, unless it fits the buffer on screen
    ///   at the same scale with nothing waiting, when it is acknowledged and
    ///   committed at once. With something waiting, a commit now could carry
    ///   what a failed present left on the wire.
    /// - Otherwise — off screen, or not presenting through the hook — it is
    ///   acknowledged now, with everything owed: an older configure left owed
    ///   would be acknowledged after it. Committed at once unless it resizes a
    ///   window on screen, whose commit is the caller's own next frame.
    pub(super) fn configure(
        &mut self,
        serial: u32,
        geometry: Geometry,
        window: WindowFacts,
    ) -> Option<Now<R>> {
        if window.presenting && window.on_screen {
            let fits =
                self.shown == Some(geometry.size) && !window.scale_changed && !self.resizing();
            if !fits {
                self.owe_configure(serial, geometry);
                return None;
            }
            return Some(Now {
                batch: Batch {
                    ack: Some(serial),
                    viewport: Some(geometry.logical),
                    ..Batch::empty()
                },
                commit: true,
            });
        }
        let mut batch = self.drain();
        batch.ack = Some(serial);
        batch.viewport = Some(geometry.logical);
        Some(Now {
            batch,
            commit: !(window.resized && window.on_screen),
        })
    }

    /// What a new scale does: owed to the frame drawn at it (`None`), or sent
    /// now with everything owed, as [`configure`](Self::configure) sends a
    /// configure it does not owe. `geometry` is the panel request's own when
    /// one waits, and `buffer_scale` the integer scale that goes with it where
    /// `wp_viewport` does not stand in for one: through
    /// [`drain`](Self::drain), which holds it back from a buffer it does not
    /// divide.
    pub(super) fn rescale(
        &mut self,
        geometry: Geometry,
        buffer_scale: Option<i32>,
        window: WindowFacts,
    ) -> Option<Now<R>> {
        if window.presenting && window.on_screen {
            self.owe_rescale(geometry, buffer_scale);
            return None;
        }
        if buffer_scale.is_some() {
            self.buffer_scale = buffer_scale;
        }
        let mut batch = self.drain();
        batch.viewport = Some(geometry.logical);
        Some(Now {
            batch,
            commit: !window.on_screen,
        })
    }

    /// What a [`Shell::set_panel`](crate::Shell::set_panel) for `request` at
    /// `geometry` does, of a panel `own_size`d (no stretched axis).
    ///
    /// Owed, and told to the caller at once, when the panel presents through
    /// its hook on screen with a size of its own — unless that size is the
    /// one on screen and nothing waits, where nothing could be stretched and
    /// a commit now is what lands a change of margin or keyboard on a panel
    /// that is not drawing. Everything else is sent and committed at once:
    /// a stretched axis, which is the compositor's to assign, a panel not on
    /// screen, one not presenting through its hook. Sent at once, it
    /// withdraws a request still waiting.
    pub(super) fn set_panel(
        &mut self,
        request: OwedPanel,
        geometry: Geometry,
        own_size: bool,
        window: WindowFacts,
    ) -> PanelReply<R> {
        let waits = window.presenting && window.on_screen && own_size;
        let unchanged = self.shown == Some(geometry.size) && !self.resizing();
        if waits && !unchanged {
            self.owe_panel(request, geometry);
            return PanelReply::Owed;
        }
        let input_region = if self.panel.is_some() {
            self.abandon_panel()
        } else {
            None
        };
        PanelReply::AtOnce { input_region }
    }

    /// A configure that resizes the window, acknowledged with the frame at
    /// `geometry`, which becomes the target.
    ///
    /// While a panel request waits, `geometry` is the request's own at the
    /// configure's scale (see [`logical_for`](Self::logical_for)), so the
    /// configure is acknowledged with the request's frame.
    fn owe_configure(&mut self, serial: u32, geometry: Geometry) {
        if self.configures.len() == MAX_OWED_CONFIGURES {
            self.configures.remove(0);
        }
        self.configures.push((serial, geometry));
        self.target = Some(geometry);
    }

    /// A new scale: the window's geometry at it, and the integer buffer scale
    /// that goes with it where `wp_viewport` does not stand in for one.
    /// `geometry` is the panel request's own when one waits.
    fn owe_rescale(&mut self, geometry: Geometry, buffer_scale: Option<i32>) {
        self.target = Some(geometry);
        self.rescaled = Some(geometry);
        if buffer_scale.is_some() {
            self.buffer_scale = buffer_scale;
        }
    }

    /// A panel's own new size, which the caller has been told and draws at.
    /// Replaces any request still waiting.
    fn owe_panel(&mut self, panel: OwedPanel, geometry: Geometry) {
        self.panel = Some((panel, geometry));
        self.target = Some(geometry);
    }

    /// Withdraws a waiting panel request, replaced by one committed at once.
    /// The target falls back to what else waits: the newest configure, else a
    /// rescale, else nothing; with nothing, the input region waiting goes
    /// with that commit and is handed back to send.
    fn abandon_panel(&mut self) -> Option<R> {
        self.panel = None;
        self.target = self
            .configures
            .last()
            .map(|(_, geometry)| *geometry)
            .or(self.rescaled);
        if self.target.is_none() {
            return self.input_region.take();
        }
        None
    }

    /// The caller was handed `size` in a `Resized` or `ScaleFactorChanged`:
    /// while that is the target, the next frame carries everything owed,
    /// whatever its size; see the [module docs](self).
    ///
    /// Kept after that frame: a target the caller is later given again at
    /// the same size, with no new `Resized` because its size did not change,
    /// is one it already knows — an immediate-mode caller asking for its
    /// size every frame, a compositor restating a configure.
    pub(super) fn told(&mut self, size: PhysicalSize) {
        self.told = Some(size);
    }

    /// An input region for the new layout, sent with the frame at the target.
    /// Hands back the one it replaces, for the caller to destroy.
    pub(super) fn owe_input_region(&mut self, region: R) -> Option<R> {
        self.input_region.replace(region)
    }

    /// What the frame drawn at `frame` carries, taken out of what is owed.
    ///
    /// - At the target, or at any size once the caller has been
    ///   [told](Self::told) the target: everything, the newest configure's
    ///   acknowledgement among it, and the target's viewport destination.
    /// - Otherwise, at the size of a configure still waiting: that one's
    ///   acknowledgement and destination, which acknowledge every older one
    ///   too.
    /// - Otherwise nothing, so the frame keeps the geometry it was drawn for.
    ///
    /// Any of them takes a buffer scale held back from an earlier frame, if
    /// this one's size divides it. The frame becomes the size shown.
    pub(super) fn take_for_frame(&mut self, frame: PhysicalSize) -> Batch<R> {
        self.before = self.shown;
        self.shown = Some(frame);
        self.in_flight = None;
        let target = self.target;
        let scale_before = self.scale;
        let due =
            target.is_some_and(|target| target.size == frame || self.told == Some(target.size));
        let mut batch = if due {
            self.drain()
        } else if let Some(index) = self
            .configures
            .iter()
            .rposition(|(_, geometry)| geometry.size == frame)
        {
            let (serial, geometry) = self.configures[index];
            self.configures.drain(..=index);
            Batch {
                ack: Some(serial),
                viewport: Some(geometry.logical),
                ..Batch::empty()
            }
        } else {
            Batch::empty()
        };
        if batch.buffer_scale.is_none() {
            batch.buffer_scale = self.take_scale();
        }
        let target = target.filter(|_| due);
        let scale = batch.buffer_scale.map(|scale| (scale, scale_before));
        if target.is_some() || scale.is_some() {
            self.in_flight = Some(InFlight { target, scale });
        }
        batch
    }

    /// The buffer scale owed, if the buffer shown takes it; it is then the
    /// one in effect.
    fn take_scale(&mut self) -> Option<i32> {
        let scale = self
            .buffer_scale
            .filter(|scale| divides(self.shown, *scale))?;
        self.buffer_scale = None;
        self.scale = Some(scale);
        Some(scale)
    }

    /// Everything owed, whatever size it waits for: what a due frame
    /// carries, and what goes with an acknowledgement made at once for a
    /// window off screen or not presenting, which replaces the batch's own
    /// with the newer serial. Nothing waits after it but a buffer scale the
    /// buffer shown does not divide, which goes with the next frame it does.
    fn drain(&mut self) -> Batch<R> {
        let batch = Batch {
            panel: self.panel.take().map(|(panel, _)| panel),
            ack: self.configures.last().map(|(serial, _)| *serial),
            buffer_scale: self.take_scale(),
            viewport: self.target.map(|target| target.logical),
            input_region: self.input_region.take(),
        };
        self.configures.clear();
        self.target = None;
        self.rescaled = None;
        batch
    }

    /// The present announced by the last frame failed, so what it sent is on
    /// the wire for whatever commits next, and the buffer up is the one
    /// before, which is the size shown again. The frame's target is the
    /// target again, so the shell owes what it would commit at once rather
    /// than commit it, until a frame lands; and a buffer scale the frame
    /// sent is owed again, with the one in effect before it handed back to
    /// send at once over it, since a commit of it on the old buffer may be
    /// `invalid_size`. Nothing else is sent again: the acknowledgement it
    /// carried went out, and acknowledging it twice is a protocol error.
    pub(super) fn present_failed(&mut self) -> Option<i32> {
        self.shown = self.before;
        let in_flight = self.in_flight.take()?;
        self.target = self.target.or(in_flight.target);
        let (sent, before) = in_flight.scale?;
        self.buffer_scale = self.buffer_scale.or(Some(sent));
        self.scale = before;
        Some(before.unwrap_or(DEFAULT_BUFFER_SCALE))
    }

    /// Stops presenting through the hook: everything owed, to send now and
    /// commit. From here the window is one that never presented through a
    /// hook, until a frame comes through one again.
    ///
    /// A buffer scale the buffer shown does not divide is dropped, not kept:
    /// no frame of the hook's will come to carry it, and the next scale
    /// change sends one as it comes. So is any, for a window whose hook was
    /// called off the shell's thread (`foreign`): the frames it presented
    /// there were never seen, so the size shown says nothing of the buffer
    /// up.
    pub(super) fn give_up(&mut self, foreign: bool) -> Batch<R> {
        if foreign {
            self.buffer_scale = None;
        }
        let batch = self.drain();
        self.buffer_scale = None;
        self.shown = None;
        self.before = None;
        self.in_flight = None;
        batch
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Batch, Geometry, MAX_OWED_CONFIGURES, Now, Owed, OwedPanel, PanelReply, WindowFacts,
    };
    use crate::role::KeptPanel;
    use crate::{LogicalSize, PanelDesc, PhysicalSize};

    const BIG: PhysicalSize = PhysicalSize::new(400, 300);
    const SMALL: PhysicalSize = PhysicalSize::new(400, 120);
    const OTHER: PhysicalSize = PhysicalSize::new(200, 100);
    /// An integer scale of two does not divide this buffer's size.
    const ODD: PhysicalSize = PhysicalSize::new(401, 301);

    fn at(size: PhysicalSize) -> Geometry {
        Geometry {
            size,
            logical: LogicalSize::new(f64::from(size.width), f64::from(size.height)),
        }
    }

    fn panel(size: PhysicalSize) -> OwedPanel {
        OwedPanel {
            size: at(size).logical,
            desc: KeptPanel::new(&PanelDesc::default()),
            set_layer: true,
        }
    }

    /// A window presenting through its hook, a frame at `shown` up.
    fn shown_at(shown: PhysicalSize) -> Owed<u32> {
        let mut owed = Owed::default();
        assert!(owed.take_for_frame(shown).is_empty());
        owed
    }

    const ON_SCREEN: WindowFacts = WindowFacts {
        presenting: true,
        on_screen: true,
        scale_changed: false,
        resized: true,
    };
    const OFF_SCREEN: WindowFacts = WindowFacts {
        on_screen: false,
        ..ON_SCREEN
    };
    const NOT_PRESENTING: WindowFacts = WindowFacts {
        presenting: false,
        ..ON_SCREEN
    };

    #[test]
    fn a_window_presents_through_its_hook_from_its_first_frame() {
        let mut owed: Owed<u32> = Owed::default();
        assert!(!owed.presenting());
        assert_eq!(owed.shown, None);
        assert!(owed.take_for_frame(BIG).is_empty());
        assert!(owed.presenting());
        assert_eq!(owed.shown, Some(BIG));
    }

    #[test]
    fn a_frame_at_the_target_carries_everything_owed() {
        let mut owed = shown_at(BIG);
        owed.owe_configure(7, at(SMALL));
        owed.owe_rescale(at(SMALL), Some(2));
        assert_eq!(owed.owe_input_region(3), None);
        assert!(owed.resizing());
        assert_eq!(
            owed.take_for_frame(SMALL),
            Batch {
                panel: None,
                ack: Some(7),
                buffer_scale: Some(2),
                viewport: Some(at(SMALL).logical),
                input_region: Some(3),
            }
        );
        assert!(!owed.resizing(), "nothing is left waiting");
        assert!(owed.take_for_frame(SMALL).is_empty(), "taken once");
    }

    #[test]
    fn a_frame_before_the_caller_is_told_carries_nothing() {
        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.owe_input_region(3);
        // Several frames a pump, before the pump that hands out `Resized`.
        for _ in 0..4 {
            assert!(owed.take_for_frame(BIG).is_empty());
        }
        assert!(owed.resizing(), "still waiting for its frame");
        assert_eq!(owed.take_for_frame(SMALL).panel, Some(panel(SMALL)));
    }

    #[test]
    fn the_first_frame_after_the_caller_is_told_carries_everything() {
        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.told(BIG);
        assert!(
            owed.take_for_frame(BIG).is_empty(),
            "told another size, not the one waited for"
        );
        owed.told(SMALL);
        assert_eq!(
            owed.take_for_frame(BIG).panel,
            Some(panel(SMALL)),
            "a caller still at the old size gets its frame scaled into the new one"
        );
        assert!(!owed.resizing());

        // An immediate-mode caller asking for the same size every frame, and
        // ignoring `Resized`, is not held back by asking again.
        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.told(SMALL);
        owed.owe_panel(panel(SMALL), at(SMALL));
        assert!(!owed.take_for_frame(BIG).is_empty());

        // A target that falls back to one the caller was not told waits again.
        let mut owed = shown_at(BIG);
        owed.owe_configure(1, at(OTHER));
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.told(SMALL);
        owed.abandon_panel();
        assert!(owed.take_for_frame(BIG).is_empty());
    }

    #[test]
    fn a_frame_at_another_size_still_becomes_the_size_shown() {
        let mut owed = shown_at(BIG);
        owed.owe_configure(1, at(SMALL));
        assert!(owed.take_for_frame(OTHER).is_empty());
        assert_eq!(owed.shown, Some(OTHER));
    }

    #[test]
    fn a_failed_present_shows_the_frame_before_again() {
        let mut owed = shown_at(BIG);
        owed.owe_configure(1, at(SMALL));
        assert!(!owed.take_for_frame(SMALL).is_empty());
        owed.present_failed();
        assert_eq!(owed.shown, Some(BIG), "the buffer up is the old one");
        // A configure at the failed frame's size does not fit the buffer up,
        // so its acknowledgement waits rather than committing what the failed
        // frame sent onto the old buffer.
        assert_eq!(owed.configure(2, at(SMALL), ON_SCREEN), None);
    }

    #[test]
    fn a_frame_at_an_older_configure_acknowledges_it_and_the_newer_wait() {
        let mut owed = shown_at(BIG);
        owed.owe_configure(1, at(OTHER));
        owed.owe_configure(2, at(SMALL));
        owed.owe_configure(3, at(OTHER));
        owed.owe_configure(4, at(PhysicalSize::new(500, 500)));
        assert_eq!(
            owed.take_for_frame(OTHER),
            Batch {
                ack: Some(3),
                viewport: Some(at(OTHER).logical),
                ..Batch::empty()
            },
            "the newest configure at the frame's size, which acknowledges the older"
        );
        assert!(owed.resizing(), "the newest still waits");
        assert!(
            owed.take_for_frame(SMALL).is_empty(),
            "configure 2 was acknowledged by 3"
        );
        assert_eq!(
            owed.take_for_frame(PhysicalSize::new(500, 500)).ack,
            Some(4)
        );
    }

    #[test]
    fn a_configure_on_screen_that_resizes_is_owed() {
        let mut owed = shown_at(BIG);
        assert_eq!(owed.configure(5, at(SMALL), ON_SCREEN), None);
        assert_eq!(owed.take_for_frame(SMALL).ack, Some(5));
    }

    #[test]
    fn a_configure_that_fits_is_acked_at_once_only_with_nothing_waiting() {
        let mut owed = shown_at(BIG);
        assert_eq!(
            owed.configure(5, at(BIG), ON_SCREEN),
            Some(Now {
                batch: Batch {
                    ack: Some(5),
                    viewport: Some(at(BIG).logical),
                    ..Batch::empty()
                },
                commit: true,
            })
        );

        // With a new size waiting, it waits too, and the frame it fits
        // acknowledges both, newest first and only.
        assert_eq!(owed.configure(6, at(SMALL), ON_SCREEN), None);
        assert_eq!(owed.configure(7, at(BIG), ON_SCREEN), None);
        assert_eq!(owed.take_for_frame(BIG).ack, Some(7));
        assert!(!owed.resizing());
    }

    #[test]
    fn a_configure_acked_at_once_off_screen_takes_every_older_one() {
        for (what, window) in [
            ("off screen", OFF_SCREEN),
            ("not presenting", NOT_PRESENTING),
        ] {
            let mut owed = shown_at(BIG);
            owed.configure(5, at(SMALL), ON_SCREEN);
            owed.owe_rescale(at(SMALL), Some(2));
            let now = owed
                .configure(6, at(OTHER), window)
                .expect("acknowledged at once");
            assert_eq!(now.batch.ack, Some(6), "{what}");
            assert_eq!(now.batch.viewport, Some(at(OTHER).logical), "{what}");
            assert_eq!(
                now.batch.buffer_scale,
                Some(2),
                "{what}: what was owed goes too"
            );
            assert!(!owed.resizing(), "{what}");
            assert!(
                owed.take_for_frame(SMALL).is_empty(),
                "{what}: configure 5 is not acknowledged after 6"
            );
        }
    }

    #[test]
    fn a_configure_acked_at_once_commits_unless_it_resizes_a_window_on_screen() {
        let mut owed: Owed<u32> = Owed::default();
        let commit = |owed: &mut Owed<u32>, window| {
            owed.configure(1, at(BIG), window)
                .expect("acknowledged at once")
                .commit
        };
        assert!(
            !commit(&mut owed, NOT_PRESENTING),
            "the caller's next frame"
        );
        assert!(commit(&mut owed, OFF_SCREEN));
        assert!(commit(
            &mut owed,
            WindowFacts {
                resized: false,
                ..NOT_PRESENTING
            }
        ));
    }

    #[test]
    fn a_rescale_on_screen_is_owed_and_off_screen_takes_what_was_owed() {
        let mut owed = shown_at(BIG);
        assert_eq!(owed.rescale(at(SMALL), Some(2), ON_SCREEN), None);
        assert!(owed.resizing());

        // A window off screen: one that left its last output, whose integer
        // scale falls back to one as it goes.
        let mut owed = shown_at(BIG);
        owed.configure(5, at(SMALL), ON_SCREEN);
        let now = owed
            .rescale(at(OTHER), Some(1), OFF_SCREEN)
            .expect("sent at once");
        assert_eq!(now.batch.ack, Some(5), "the newest owed goes with it");
        assert_eq!(now.batch.buffer_scale, Some(1));
        assert_eq!(now.batch.viewport, Some(at(OTHER).logical));
        assert!(now.commit, "off screen, nothing waits for a frame");
        assert!(!owed.resizing());
        assert!(owed.take_for_frame(SMALL).is_empty());
    }

    #[test]
    fn an_integer_scale_is_never_sent_with_a_buffer_it_does_not_divide() {
        // The frame after the caller was told, drawn at an odd size.
        let mut owed = shown_at(ODD);
        owed.owe_rescale(at(SMALL), Some(2));
        owed.told(SMALL);
        let batch = owed.take_for_frame(ODD);
        assert_eq!(batch.viewport, Some(at(SMALL).logical), "it is due");
        assert_eq!(batch.buffer_scale, None, "but not with the scale");
        assert!(
            !owed.resizing(),
            "which holds nothing else up: an input region goes at once from here"
        );
        assert!(
            owed.take_for_frame(ODD).is_empty(),
            "nor goes with an odd frame"
        );
        assert_eq!(
            owed.take_for_frame(SMALL).buffer_scale,
            Some(2),
            "but with the first frame it divides, whatever that one is"
        );

        // A rescale sent at once with an odd-sized buffer up. No real path
        // sends one above scale one off screen — leaving the last output falls
        // back to one — so this guards the rule `drain` keeps, not a path.
        let mut owed = shown_at(ODD);
        let now = owed
            .rescale(at(SMALL), Some(2), OFF_SCREEN)
            .expect("at once");
        assert_eq!(now.batch.buffer_scale, None);
        assert!(!owed.resizing());
        assert_eq!(owed.take_for_frame(SMALL).buffer_scale, Some(2));

        // Given up with an odd-sized buffer up, it is dropped; with a buffer
        // it divides, it goes.
        let mut owed = shown_at(ODD);
        owed.owe_rescale(at(SMALL), Some(2));
        assert_eq!(owed.give_up(false).buffer_scale, None);
        assert!(!owed.resizing(), "and nothing is left owed");
        assert_eq!(
            owed.configure(2, at(BIG), NOT_PRESENTING)
                .expect("at once")
                .batch
                .buffer_scale,
            None,
            "not even for a later commit at once onto the same buffer"
        );
        let mut owed = shown_at(BIG);
        owed.owe_rescale(at(SMALL), Some(2));
        assert_eq!(owed.give_up(false).buffer_scale, Some(2));
        // Given up off the shell's thread, the size shown says nothing of the
        // buffer up, so no scale goes.
        let mut owed = shown_at(BIG);
        owed.owe_rescale(at(SMALL), Some(2));
        assert_eq!(owed.give_up(true).buffer_scale, None);

        // A window that never attached a buffer through the hook has none to
        // break.
        let mut owed: Owed<u32> = Owed::default();
        let now = owed
            .rescale(at(SMALL), Some(2), NOT_PRESENTING)
            .expect("at once");
        assert_eq!(now.batch.buffer_scale, Some(2));
    }

    #[test]
    fn a_window_given_up_sends_what_it_owed_and_presents_no_more() {
        let mut owed = shown_at(BIG);
        owed.owe_configure(1, at(SMALL));
        owed.owe_panel(panel(OTHER), at(OTHER));
        let batch = owed.give_up(false);
        assert_eq!(batch.ack, Some(1));
        assert_eq!(batch.panel, Some(panel(OTHER)));
        assert!(!owed.presenting());
        assert!(!owed.resizing());
        assert!(owed.give_up(false).is_empty(), "nothing left");
        // A configure from here is one for a window that never presented.
        assert!(owed.configure(2, at(SMALL), NOT_PRESENTING).is_some());
    }

    #[test]
    fn a_configure_while_a_panel_request_waits_rides_with_its_frame() {
        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        // The compositor configures against the request before; the shell
        // owes it at the waiting request's geometry.
        owed.owe_configure(5, at(SMALL));
        assert!(owed.take_for_frame(BIG).is_empty());
        let batch = owed.take_for_frame(SMALL);
        assert_eq!(batch.panel, Some(panel(SMALL)));
        assert_eq!(batch.ack, Some(5));
        assert_eq!(batch.viewport, Some(at(SMALL).logical));
    }

    #[test]
    fn a_later_panel_request_replaces_the_one_waiting() {
        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.owe_panel(panel(OTHER), at(OTHER));
        assert!(owed.take_for_frame(SMALL).is_empty());
        assert_eq!(owed.take_for_frame(OTHER).panel, Some(panel(OTHER)));
    }

    #[test]
    fn an_abandoned_panel_request_falls_back_to_what_else_waits() {
        let mut owed = shown_at(BIG);
        owed.owe_configure(1, at(OTHER));
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.owe_input_region(3);
        assert_eq!(owed.abandon_panel(), None, "a configure still waits");
        let batch = owed.take_for_frame(OTHER);
        assert_eq!((batch.panel, batch.ack), (None, Some(1)));
        assert_eq!(batch.input_region, Some(3));

        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.owe_rescale(at(SMALL), Some(2));
        owed.owe_panel(panel(OTHER), at(OTHER));
        assert_eq!(owed.abandon_panel(), None, "the rescale still waits");
        assert_eq!(owed.take_for_frame(SMALL).buffer_scale, Some(2));

        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.owe_input_region(3);
        assert_eq!(
            owed.abandon_panel(),
            Some(3),
            "nothing waits, so the region goes with the commit"
        );
        assert!(!owed.resizing());
    }

    #[test]
    fn a_rescale_keeps_a_buffer_scale_already_owed() {
        let mut owed = shown_at(BIG);
        owed.owe_rescale(at(SMALL), Some(2));
        owed.owe_rescale(at(SMALL), None);
        assert_eq!(owed.take_for_frame(SMALL).buffer_scale, Some(2));
    }

    #[test]
    fn a_new_region_hands_back_the_one_it_replaces() {
        let mut owed = shown_at(BIG);
        assert_eq!(owed.owe_input_region(1), None);
        assert_eq!(owed.owe_input_region(2), Some(1));
    }

    #[test]
    fn only_the_newest_configures_are_kept() {
        let mut owed = shown_at(BIG);
        let first = PhysicalSize::new(1, 1);
        owed.owe_configure(0, at(first));
        for serial in 1..=u32::try_from(MAX_OWED_CONFIGURES).expect("small") {
            owed.owe_configure(serial, at(PhysicalSize::new(serial + 1, 1)));
        }
        assert!(
            owed.take_for_frame(first).is_empty(),
            "the oldest was dropped, its acknowledgement left to a newer one"
        );
    }

    #[test]
    fn a_failed_present_makes_its_target_the_target_again() {
        // A new scale on an odd-sized buffer at scale 1, the frame at the new
        // size sending the scale, and its present failing.
        let mut owed = shown_at(ODD);
        owed.scale_sent(1);
        owed.owe_configure(4, at(SMALL));
        owed.owe_rescale(at(SMALL), Some(2));
        assert_eq!(owed.take_for_frame(SMALL).buffer_scale, Some(2));
        assert_eq!(
            owed.present_failed(),
            Some(1),
            "the scale in effect before, to send over the one on the wire"
        );
        assert_eq!(owed.shown, Some(ODD));
        assert!(owed.resizing(), "what it took waits for a frame again");
        // A configure that fits the buffer up waits rather than committing
        // what the failed frame left on the wire onto it.
        assert_eq!(owed.configure(5, at(ODD), ON_SCREEN), None);
        // The next frame at the size takes the scale again, and the newer
        // configure; the one the failed frame acknowledged is not sent again.
        let batch = owed.take_for_frame(SMALL);
        assert_eq!(batch.buffer_scale, Some(2));
        assert_eq!(batch.ack, None, "configure 5 fits another size");

        // A failed frame that sent no scale hands none back, and one that
        // carried nothing puts nothing back.
        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.take_for_frame(SMALL);
        assert_eq!(owed.present_failed(), None);
        assert!(owed.resizing());
        let mut owed = shown_at(BIG);
        owed.take_for_frame(BIG);
        assert_eq!(owed.present_failed(), None);
        assert!(!owed.resizing());
    }

    #[test]
    fn a_panel_request_waits_only_on_screen_with_a_size_of_its_own() {
        let mut owed = shown_at(BIG);
        assert_eq!(
            owed.set_panel(panel(SMALL), at(SMALL), true, ON_SCREEN),
            PanelReply::Owed
        );
        assert!(owed.resizing());

        // A stretched axis, a panel off screen and one not presenting through
        // its hook are committed at once, withdrawing what waits.
        for (what, own_size, window) in [
            ("stretched", false, ON_SCREEN),
            ("off screen", true, OFF_SCREEN),
            ("not presenting", true, NOT_PRESENTING),
        ] {
            let mut owed = shown_at(BIG);
            owed.set_panel(panel(SMALL), at(SMALL), true, ON_SCREEN);
            owed.owe_input_region(3);
            assert_eq!(
                owed.set_panel(panel(OTHER), at(OTHER), own_size, window),
                PanelReply::AtOnce {
                    input_region: Some(3)
                },
                "{what}: the region goes with the commit, nothing else waiting"
            );
            assert!(!owed.resizing(), "{what}");
        }

        // The size on screen with nothing waiting goes at once, as a margin
        // change would; with something waiting it waits.
        let mut owed = shown_at(BIG);
        assert_eq!(
            owed.set_panel(panel(BIG), at(BIG), true, ON_SCREEN),
            PanelReply::AtOnce { input_region: None }
        );
        owed.owe_configure(1, at(SMALL));
        assert_eq!(
            owed.set_panel(panel(BIG), at(BIG), true, ON_SCREEN),
            PanelReply::Owed
        );
    }

    #[test]
    fn a_configure_gives_way_to_a_panel_request_waiting() {
        let mut owed: Owed<u32> = shown_at(BIG);
        let configured = at(BIG).logical;
        assert_eq!(owed.logical_for(configured), configured);
        owed.owe_panel(panel(SMALL), at(SMALL));
        assert_eq!(owed.logical_for(configured), at(SMALL).logical);
        owed.take_for_frame(SMALL);
        assert_eq!(owed.logical_for(configured), configured, "landed");
    }

    #[test]
    fn being_told_a_size_again_is_not_needed_to_ask_for_it_again() {
        // A caller told SMALL, whose frame took the request, asks for SMALL
        // again while its buffer is still the old size: no `Resized` comes,
        // since its size did not change, and the frame after it still
        // carries the request.
        let mut owed = shown_at(BIG);
        owed.owe_panel(panel(SMALL), at(SMALL));
        owed.told(SMALL);
        assert!(!owed.take_for_frame(BIG).is_empty());
        owed.owe_panel(panel(SMALL), at(SMALL));
        assert_eq!(owed.take_for_frame(BIG).panel, Some(panel(SMALL)));
    }

    #[test]
    fn a_failed_frame_that_was_not_due_puts_its_scale_back_too() {
        // A caller ignoring `Resized` presents its due frame at an odd size,
        // which holds the scale back; its next frame, at an even size and not
        // due, takes the scale, and its present fails.
        let mut owed = shown_at(ODD);
        owed.scale_sent(1);
        owed.owe_rescale(at(SMALL), Some(2));
        owed.told(SMALL);
        assert_eq!(owed.take_for_frame(ODD).buffer_scale, None);
        assert!(!owed.resizing());
        assert_eq!(owed.take_for_frame(BIG).buffer_scale, Some(2));
        assert_eq!(
            owed.present_failed(),
            Some(1),
            "the scale in effect, sent back over the one on the wire, so a \
             commit at once of the odd buffer is safe"
        );
        assert!(!owed.resizing(), "nothing else waits, so nothing does");
        assert_eq!(
            owed.take_for_frame(BIG).buffer_scale,
            Some(2),
            "owed again, for the next frame it divides"
        );

        // The same for a frame acknowledging an older configure.
        let mut owed = shown_at(ODD);
        owed.scale_sent(1);
        owed.owe_configure(1, at(BIG));
        owed.owe_configure(2, at(SMALL));
        owed.owe_rescale(at(SMALL), Some(2));
        let batch = owed.take_for_frame(BIG);
        assert_eq!((batch.ack, batch.buffer_scale), (Some(1), Some(2)));
        assert_eq!(owed.present_failed(), Some(1));
    }
}
