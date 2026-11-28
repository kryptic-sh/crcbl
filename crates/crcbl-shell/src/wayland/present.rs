//! A window's [`PresentHook`](crate::PresentHook): sending what the frame
//! about to be presented carries.
//!
//! The present is the driver's `wl_surface.commit`, which this backend never
//! sees; [`Shell::present_hook`](crate::Shell::present_hook) is how the
//! presenter says one is coming. `will_present` asks the window's
//! [`Owed`] what a frame of that size carries and sends it, uncommitted, so
//! the present's commit that follows applies it with the frame. See
//! [`owed`](super::owed) for what waits and why.
//!
//! # One thread
//!
//! **A hook sends only on the thread that runs the shell.** libwayland would
//! marshal a request from any thread, but ordering is the whole point here:
//! what the hook sends must reach the wire after every request the shell made
//! for the frame and before the present's commit, and nothing else of the
//! shell's may slip between them and commit it early. With the shell, the
//! hook and the present on one thread that holds by construction, and nothing
//! in crcbl presents from another — the engine's loop and every sample pump,
//! draw and present on the thread that opened the shell, which the shell's
//! `!Send` requires. So the thread is checked rather than assumed: a
//! `will_present` from any other thread sends nothing, logs an error once,
//! and gives the window up.
//!
//! # Giving a window up
//!
//! A window is given up when its last hook is dropped — its presenter was
//! closed while the window stays open — or when a hook is called off the
//! shell's thread. Each [`Shell::present_hook`] call hands out a hook on a
//! [`WlHook`] of its own, shared by that hook's clones, and the window counts
//! the live ones. A given-up window is settled at the shell's next pump, or
//! at the next request the shell sends it at once, whichever comes first:
//! everything it owed is sent, in order and ahead of that request. At the
//! pump it is committed there; ahead of a request, it rides that request's
//! commit, or the window's next frame where the request leaves its commit to
//! one, as a resize on screen does. From then on it is a window that never
//! presented through a hook, until a hook presents for it again: a new hook
//! taken for it, from the shell's thread, is trusted again.
//!
//! [`Shell::present_hook`]: crate::Shell::present_hook

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

use crate::PhysicalSize;
use crate::present_hook::PresentSignal;

use super::ffi::WlProxy;
use super::layer;
use super::owed::{Batch, Now, Owed};
use super::protocol::layer_shell::zwlr_layer_surface_v1;
use super::protocol::wayland::{wl_region, wl_surface};
use super::protocol::xdg_shell::xdg_surface;
use super::set_viewport;

/// A `wl_region` an owed input region is held in, or null for "the whole
/// surface".
#[derive(Debug)]
pub(super) struct Region(pub(super) *mut WlProxy);

// SAFETY: a `Region` is a proxy address that crosses threads only as data:
// it is created, sent and destroyed on the shell's thread alone — the
// `owner` check in `WlPresent::will_present` keeps every send there, and
// `WlPresent::retire` hands what is left back to the shell to destroy.
unsafe impl Send for Region {}

/// The proxies a hook sends on: the window's, given by [`WlPresent::attach`]
/// as a hook is handed out.
#[derive(Debug)]
pub(super) struct Proxies {
    pub(super) surface: *mut WlProxy,
    pub(super) xdg_surface: *mut WlProxy,
    pub(super) layer_surface: *mut WlProxy,
    pub(super) viewport: *mut WlProxy,
}

// SAFETY: as `Region`'s: the pointers are dereferenced only by `send`, which
// runs only on the shell's thread — `WlPresent::will_present` and
// `present_failed` check `owner` first, and the shell's own calls are on its
// thread because the shell is `!Send` — and only while `Shared::proxies` is
// `Some`, which `WlPresent::retire` ends under the same lock before the
// window's proxies are destroyed.
unsafe impl Send for Proxies {}

/// What the window and its hooks share, under [`WlPresent`]'s lock.
#[derive(Debug, Default)]
pub(super) struct Shared {
    pub(super) owed: Owed<Region>,
    /// `None` once the window's objects are gone, after which the hook sends
    /// nothing.
    proxies: Option<Proxies>,
    /// Whether a hook was called from a thread other than the shell's; see
    /// the module docs.
    foreign: bool,
    /// The [`WlHook`]s alive for the window.
    hooks: usize,
}

impl Shared {
    /// Whether the window presents through its hook, so state may wait for a
    /// frame.
    pub(super) const fn presenting(&self) -> bool {
        self.owed.presenting() && !self.foreign && self.hooks > 0
    }

    /// For a window given up — see the module docs — that has not been
    /// settled yet: everything it owed, to send and commit. Nothing for one
    /// still presenting through its hook, one already settled, or one
    /// retired, which [`WlPresent::retire`] settles.
    ///
    /// **Every request the shell sends at once calls this first**, and sends
    /// what it hands back ahead of its own state, so what was owed is on the
    /// wire before what supersedes it: a panel request or input region given
    /// at once after the window's last hook went must not be followed by the
    /// older one still owed, as the next pump's settle would otherwise send it.
    pub(super) fn settle_given_up(&mut self) -> Option<Batch<Region>> {
        let given_up = self.foreign || self.hooks == 0;
        if !given_up || !self.owed.presenting() {
            return None;
        }
        Some(self.owed.give_up(self.foreign))
    }
}

/// One window's half of its [`PresentHook`](crate::PresentHook), shared
/// between the window and every hook handed out for it.
#[derive(Debug)]
pub(super) struct WlPresent {
    /// The shell's thread, the only one a hook sends on.
    owner: ThreadId,
    shared: Mutex<Shared>,
}

impl WlPresent {
    /// The hook state for a window, made on the shell's thread, with no
    /// proxies until [`attach`](Self::attach) gives it the window's.
    pub(super) fn new() -> Self {
        Self {
            owner: thread::current().id(),
            shared: Mutex::new(Shared::default()),
        }
    }

    /// Gives the hooks the window's proxies to send on, as one is handed out.
    pub(super) fn attach(&self, proxies: Proxies) {
        self.lock().proxies = Some(proxies);
    }

    /// Whether the call is on the shell's thread; a call from any other gives
    /// the window up, saying so once.
    fn on_owner_thread(&self, shared: &mut Shared) -> bool {
        if thread::current().id() == self.owner {
            return true;
        }
        if !shared.foreign {
            shared.foreign = true;
            crcbl_core::log::error!(
                "wayland: a present hook was called off the shell's thread; the window's \
                 size changes no longer wait for their frames"
            );
        }
        false
    }

    /// The shared state, for the shell to record into.
    pub(super) fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The window's objects are about to be destroyed: no hook may send on
    /// them again. Hands back what is owed, for the shell to free the region
    /// in it.
    pub(super) fn retire(&self) -> Batch<Region> {
        let mut shared = self.lock();
        shared.proxies = None;
        shared.owed.give_up(false)
    }

    /// [`Shared::settle_given_up`] for the shell's pump: sends what it hands
    /// back, and says whether the caller must commit it.
    pub(super) fn settle_given_up(&self) -> bool {
        settle(&mut self.lock())
    }

    /// The same, for a window given up off the shell's thread only, as a new
    /// hook is taken for it: what it owed goes, and its scale is dropped,
    /// before that hook's frames are taken at their word again. Whether the
    /// caller must commit it.
    pub(super) fn settle_foreign(&self) -> bool {
        let mut shared = self.lock();
        shared.foreign && settle(&mut shared)
    }

    fn will_present(&self, size: PhysicalSize) {
        let mut shared = self.lock();
        if shared.foreign || !self.on_owner_thread(&mut shared) {
            return;
        }
        let shared = &mut *shared;
        let Some(proxies) = &shared.proxies else {
            return;
        };
        let batch = shared.owed.take_for_frame(size);
        // SAFETY: on the shell's thread, just checked, with the proxies still
        // live: `proxies` is `Some`.
        unsafe { send(proxies, batch) };
    }

    /// See [`Owed::present_failed`]: sends back the buffer scale in effect
    /// over one the failed frame sent.
    fn present_failed(&self) {
        let mut shared = self.lock();
        if shared.foreign || !self.on_owner_thread(&mut shared) {
            return;
        }
        let shared = &mut *shared;
        let Some(scale) = shared.owed.present_failed() else {
            return;
        };
        let Some(proxies) = &shared.proxies else {
            return;
        };
        // SAFETY: on the shell's thread, just checked, with the surface still
        // live: `proxies` is `Some`. `set_buffer_scale` takes one int.
        unsafe { wl_surface::set_buffer_scale(proxies.surface, scale) };
    }
}

/// What every [`PresentHook`](crate::PresentHook) handed out for one
/// [`Shell::present_hook`](crate::Shell::present_hook) call shares: the
/// window's [`WlPresent`], counted live for as long as one of them is.
#[derive(Debug)]
pub(super) struct WlHook(Arc<WlPresent>);

impl WlHook {
    /// A hook for `present`'s window, taken on the shell's thread: a window
    /// given up for a hook called off it is trusted again, and presents
    /// through this one from its first frame.
    ///
    /// Take one only once a window given up off the thread is settled
    /// ([`WlPresent::settle_foreign`]), as `Shell::present_hook` does: this
    /// trusts the window's frames again.
    pub(super) fn new(present: Arc<WlPresent>) -> Self {
        let mut shared = present.lock();
        shared.foreign = false;
        shared.hooks += 1;
        drop(shared);
        Self(present)
    }
}

impl Drop for WlHook {
    fn drop(&mut self) {
        let mut shared = self.0.lock();
        shared.hooks = shared.hooks.saturating_sub(1);
    }
}

impl PresentSignal for WlHook {
    fn will_present(&self, size: PhysicalSize) {
        self.0.will_present(size);
    }

    /// Nothing waits on a present's outcome: what the frame carried went
    /// with its commit. Nothing changes, so no thread check is needed.
    fn presented(&self) {}

    fn present_failed(&self) {
        self.0.present_failed();
    }
}

/// Settles a given-up window's state on its own proxies; see
/// [`Shared::settle_given_up`]. Whether the caller must commit it.
///
/// On the shell's thread only: every caller is a shell method or the pump.
fn settle(shared: &mut Shared) -> bool {
    let Some(batch) = shared.settle_given_up() else {
        return false;
    };
    // Retired windows are settled by `retire`, so a live one has these.
    let Some(proxies) = &shared.proxies else {
        return false;
    };
    // SAFETY: on the shell's thread, with the proxies still live: `proxies`
    // is `Some`.
    unsafe { send(proxies, batch) };
    true
}

/// Sends what [`Shared::settle_given_up`] handed back, if anything, ahead of
/// the request that called it, which commits it with its own state.
///
/// # Safety
///
/// As [`send`].
pub(super) unsafe fn send_settled(proxies: &Proxies, settled: Option<Batch<Region>>) {
    if let Some(settled) = settled {
        // SAFETY: the caller's guarantees are `send`'s.
        unsafe { send(proxies, settled) };
    }
}

/// Sends `now`'s batch on `proxies`, and commits it if it says so.
///
/// # Safety
///
/// As [`send`].
pub(super) unsafe fn send_now(proxies: &Proxies, now: Now<Region>) {
    // SAFETY: the caller's guarantees are `send`'s, and the surface is one of
    // the proxies they cover.
    unsafe {
        send(proxies, now.batch);
        if now.commit {
            wl_surface::commit(proxies.surface);
        }
    }
}

/// Sends `batch` on `proxies`, uncommitted.
///
/// # Safety
///
/// On the shell's thread, with every proxy in `proxies` null or live, and the
/// region in `batch` null or a live `wl_region` of this connection, which is
/// destroyed here.
pub(super) unsafe fn send(proxies: &Proxies, batch: Batch<Region>) {
    // SAFETY: the caller guarantees the thread and every proxy; each request
    // goes to the role object that is not null, and takes plain integers.
    unsafe {
        if let Some(panel) = batch.panel
            && !proxies.layer_surface.is_null()
        {
            let desc = panel.desc.desc();
            layer::send_state(proxies.layer_surface, &desc, panel.size);
            if panel.set_layer {
                zwlr_layer_surface_v1::set_layer(
                    proxies.layer_surface,
                    layer::layer_of(desc.layer),
                );
            }
        }
        if let Some(serial) = batch.ack {
            if proxies.layer_surface.is_null() {
                xdg_surface::ack_configure(proxies.xdg_surface, serial);
            } else {
                zwlr_layer_surface_v1::ack_configure(proxies.layer_surface, serial);
            }
        }
        if let Some(scale) = batch.buffer_scale {
            wl_surface::set_buffer_scale(proxies.surface, scale);
        }
        if let Some(logical) = batch.viewport {
            set_viewport(proxies.viewport, logical);
        }
        if let Some(Region(region)) = batch.input_region {
            wl_surface::set_input_region(proxies.surface, region);
            if !region.is_null() {
                wl_region::destroy(region);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use core::ptr;
    use std::sync::Arc;

    use super::{Proxies, WlHook, WlPresent};
    use crate::present_hook::PresentSignal;
    use crate::wayland::owed::{Geometry, WindowFacts};
    use crate::{LogicalSize, PhysicalSize};

    const SHOWN: PhysicalSize = PhysicalSize::new(400, 300);
    const NEW: PhysicalSize = PhysicalSize::new(400, 120);

    /// A window's hook state with no proxies behind it. Only a viewport
    /// destination is ever owed below, which `set_viewport` skips for a null
    /// viewport, so nothing is marshalled.
    fn window() -> Arc<WlPresent> {
        let present = WlPresent::new();
        present.attach(Proxies {
            surface: ptr::null_mut(),
            xdg_surface: ptr::null_mut(),
            layer_surface: ptr::null_mut(),
            viewport: ptr::null_mut(),
        });
        Arc::new(present)
    }

    /// A window presenting through `hook`, a frame at `SHOWN` up and a new
    /// size owed.
    fn presenting(present: &Arc<WlPresent>, hook: &WlHook) {
        hook.will_present(SHOWN);
        hook.presented();
        owe_new_size(present);
    }

    fn owe_new_size(present: &WlPresent) {
        let geometry = Geometry {
            size: NEW,
            logical: LogicalSize::new(400.0, 120.0),
        };
        let on_screen = WindowFacts {
            presenting: true,
            on_screen: true,
            ..WindowFacts::default()
        };
        assert!(
            present
                .lock()
                .owed
                .rescale(geometry, None, on_screen)
                .is_none(),
            "owed, not sent"
        );
    }

    #[test]
    fn a_hook_on_the_shells_thread_takes_what_its_frame_carries() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        assert!(present.lock().presenting());
        hook.will_present(NEW);
        assert!(
            !present.lock().owed.resizing(),
            "the frame at the new size took it"
        );
    }

    #[test]
    fn a_failed_present_puts_its_frame_back() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        hook.will_present(NEW);
        assert!(!present.lock().owed.resizing());
        hook.present_failed();
        assert!(
            present.lock().owed.resizing(),
            "what the frame took waits for a frame again"
        );
    }

    #[test]
    fn a_failed_present_reported_off_the_thread_changes_nothing() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        hook.will_present(NEW);
        std::thread::scope(|scope| {
            scope.spawn(|| hook.present_failed());
        });
        let mut shared = present.lock();
        assert!(
            !shared.owed.resizing(),
            "nothing was put back off the thread"
        );
        assert!(!shared.presenting(), "and the window is given up");
        assert!(shared.settle_given_up().is_some());
        drop(shared);
        // Reported again off the thread after the give-up, it does not make
        // the window present again.
        std::thread::scope(|scope| {
            scope.spawn(|| hook.present_failed());
        });
        assert!(!present.lock().owed.presenting());
    }

    #[test]
    fn a_new_hook_settles_a_window_given_up_off_the_thread_first() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        std::thread::scope(|scope| {
            scope.spawn(|| hook.will_present(NEW));
        });
        assert!(present.lock().owed.resizing(), "owed, not yet settled");
        // A hook taken before any pump settled it, as `present_hook` takes
        // one.
        assert!(present.settle_foreign(), "settled, for a commit");
        let _reopened = WlHook::new(Arc::clone(&present));
        assert!(!present.settle_foreign(), "and only once");
        let shared = present.lock();
        assert!(!shared.owed.resizing(), "what it owed went first");
        assert!(
            !shared.owed.presenting(),
            "and the size shown off the thread is not taken at its word"
        );
    }

    #[test]
    fn a_new_hook_trusts_a_window_given_up_off_the_thread_again() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        std::thread::scope(|scope| {
            scope.spawn(|| hook.will_present(NEW));
        });
        assert!(present.settle_given_up());
        let reopened = WlHook::new(Arc::clone(&present));
        reopened.will_present(SHOWN);
        assert!(present.lock().presenting());
    }

    #[test]
    fn a_hook_off_the_shells_thread_sends_nothing_and_gives_the_window_up() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        std::thread::scope(|scope| {
            scope.spawn(|| hook.will_present(NEW));
        });
        {
            let shared = present.lock();
            assert!(shared.owed.resizing(), "nothing was taken off the thread");
            assert!(
                !shared.presenting(),
                "the window no longer waits for frames"
            );
        }
        assert!(
            present.settle_given_up(),
            "the shell sends what was owed, for a commit"
        );
        assert!(!present.lock().owed.resizing());
        assert!(!present.settle_given_up(), "and only once");
        // Called on the right thread again, it stays given up, taking nothing.
        hook.will_present(NEW);
        assert!(!present.lock().owed.presenting());
    }

    #[test]
    fn a_window_whose_last_hook_drops_is_given_up_until_a_new_one_presents() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        let second = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        drop(hook);
        assert!(present.lock().presenting(), "one hook is still alive");
        assert!(!present.settle_given_up());
        drop(second);
        assert!(!present.lock().presenting(), "none is left");
        assert!(present.settle_given_up(), "what was owed goes at once");
        assert!(!present.lock().owed.resizing());
        assert!(!present.settle_given_up(), "and only once");

        // A new presenter's first frame makes the window present again.
        let reopened = WlHook::new(Arc::clone(&present));
        assert!(!present.lock().presenting(), "until it presents");
        reopened.will_present(SHOWN);
        assert!(present.lock().presenting());
    }

    #[test]
    fn a_request_at_once_for_a_given_up_window_takes_what_it_owed_first() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        drop(hook);
        // What `set_panel` and `set_input_region` do at once: settle first,
        // and send what comes back ahead of their own state.
        let settled = present.lock().settle_given_up().expect("it owed a size");
        assert_eq!(settled.viewport, Some(LogicalSize::new(400.0, 120.0)));
        assert!(!present.lock().owed.resizing());
        assert!(
            !present.settle_given_up(),
            "the next pump has nothing older to send after the newer request"
        );
    }

    #[test]
    fn a_retired_hook_sends_nothing() {
        let present = window();
        let hook = WlHook::new(Arc::clone(&present));
        presenting(&present, &hook);
        assert!(
            present.retire().viewport.is_some(),
            "what was owed is handed back"
        );
        owe_new_size(&present);
        hook.will_present(NEW);
        assert!(
            present.lock().owed.resizing(),
            "a frame after retirement takes nothing, with nothing to send it on"
        );
        drop(hook);
        assert!(
            !present.settle_given_up(),
            "nor is anything sent for a retired window given up"
        );
    }
}
