//! Accessibility: each window's tree handed to UI Automation through
//! `accesskit_windows`. Behind the `accesskit` feature.
//!
//! # Where the adapter lives
//!
//! One [`Adapter`] per window, created right after `CreateWindowExW` and kept
//! in [`Shared`](super::proc::Shared) beside the other state the window
//! procedure reads, because two of the three things that drive it arrive there:
//! `WM_GETOBJECT`, which is a client asking for the window's provider, and
//! `WM_SETFOCUS`/`WM_KILLFOCUS`, which the adapter needs to place UI
//! Automation's focus. The third, a tree from the program, comes through
//! [`Shell::update_accessibility`](crate::Shell::update_accessibility). The
//! adapter goes with its window: [`Shared::forget`](super::proc::Shared::forget)
//! drops it, which both `destroy_window` (before `DestroyWindow`) and
//! `WM_NCDESTROY` call, so nothing reaches it after the window is gone.
//!
//! # The borrow is released before anything can re-enter
//!
//! The table is a `RefCell` like every other table in `Shared`, and the same
//! rule holds — see [`proc`](super::proc). Here it binds harder than usual,
//! because UI Automation calls back into the window procedure from the very
//! calls the adapter asks for: returning a provider from `WM_GETOBJECT` may
//! send a nested `WM_GETOBJECT`, and raising the events an update produced may
//! too. So each method here takes the borrow, asks the adapter for a value that
//! does not hold it, lets the borrow go, and only then makes the call that can
//! re-enter. Callers never see a borrow or a
//! [`QueuedEvents`](accesskit_windows::QueuedEvents).
//!
//! # Requests cross threads
//!
//! The adapter documents that its action handler may be called on another
//! thread — UI Automation's own, while the window's thread is asleep in
//! [`wait_events`](crate::Shell::wait_events). So a request goes onto
//! [`Actions`], a mutex-guarded queue the shell drains in its pump, and the
//! handler then rings the shell's [`ShellWaker`] so the wait returns and the
//! request is delivered now rather than at the next input.
//!
//! # Windows never says a reader stopped
//!
//! UI Automation has no "the last client went away" notification, so
//! [`AccessibilityDeactivated`](crate::ShellEvent::AccessibilityDeactivated) is
//! never raised on this backend. Once a reader has asked for a window, the
//! adapter stays active and every tree sent for it is kept, for the window's
//! lifetime.

use core::cell::RefCell;
use std::sync::{Arc, Mutex, PoisonError};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_windows::{Adapter, HWND, LPARAM, LRESULT, WPARAM};

use super::ffi::{Handle, Lparam, Lresult, Wparam};
use super::proc::key;
use crate::ShellWaker;

/// What readers asked for, by window key, waiting for the pump.
///
/// Shared between the shell and every window's action handler, which may run
/// on another thread — hence the mutex rather than the `RefCell` the rest of
/// this backend uses.
#[derive(Clone, Debug, Default)]
struct Actions(Arc<Mutex<Vec<(isize, ActionRequest)>>>);

impl Actions {
    fn push(&self, hwnd: isize, request: ActionRequest) {
        // A poisoned lock means a panic elsewhere while pushing or taking, and
        // the vector is still whole: no request is half-written.
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((hwnd, request));
    }

    /// Takes every request made since the last call, oldest first.
    fn take(&self) -> Vec<(isize, ActionRequest)> {
        core::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Drops the requests still waiting for a window that is going away.
    fn forget(&self, hwnd: isize) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|&(known, _)| known != hwnd);
    }
}

/// One window's action handler: queue the request, ring the doorbell.
struct Handler {
    hwnd: isize,
    actions: Actions,
    waker: Option<ShellWaker>,
}

impl ActionHandler for Handler {
    fn do_action(&mut self, request: ActionRequest) {
        self.actions.push(self.hwnd, request);
        if let Some(waker) = &self.waker {
            waker.wake();
        }
    }
}

/// The activation handler for one `WM_GETOBJECT`: notes that a reader asked,
/// and offers no tree.
///
/// `None` is deliberate. The tree belongs to the program, which hears
/// [`AccessibilityActivated`](crate::ShellEvent::AccessibilityActivated) and
/// sends a whole one through
/// [`update_accessibility`](crate::Shell::update_accessibility); until it does,
/// the adapter shows the reader a placeholder root. The adapter asks only while
/// it is inactive, so `asked` is true on exactly one message per window.
#[derive(Default)]
struct Activation {
    asked: bool,
}

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.asked = true;
        None
    }
}

/// What a `WM_GETOBJECT` came to.
pub(super) struct GetObject {
    /// The answer, or `None` for the default handler — a window with no
    /// adapter, or an object id that is not the client area's.
    pub lresult: Option<Lresult>,
    /// Whether this was the first time a reader asked for the window.
    pub activated: bool,
}

/// Every window's adapter, by window key, and the requests their readers
/// made. See the [module docs](self).
#[derive(Debug, Default)]
pub(super) struct Adapters {
    adapters: RefCell<Vec<(isize, Adapter)>>,
    actions: Actions,
}

impl Adapters {
    /// Gives `hwnd` an adapter, whose requests ring `waker` once queued.
    ///
    /// Called right after `CreateWindowExW` and never from the window
    /// procedure: [`Adapter::new`] initialises UI Automation, which must have
    /// happened before the first `WM_GETOBJECT` is answered. One arriving
    /// before this falls to the default handler. `focused` is whether the
    /// window already has the focus; its `WM_SETFOCUS`/`WM_KILLFOCUS` say from
    /// here on.
    pub(super) fn open(&self, hwnd: Handle, focused: bool, waker: Option<ShellWaker>) {
        let window = key(hwnd);
        let handler = Handler {
            hwnd: window,
            actions: self.actions.clone(),
            waker,
        };
        let adapter = Adapter::new(HWND(hwnd), focused, handler);
        self.adapters.borrow_mut().push((window, adapter));
    }

    /// Takes every request readers made since the last call, oldest first.
    pub(super) fn take_actions(&self) -> Vec<(isize, ActionRequest)> {
        self.actions.take()
    }

    /// Drops `hwnd`'s adapter, outside the borrow, and the requests still
    /// waiting for it, so a new window given the same `HWND` value inherits
    /// neither.
    pub(super) fn close(&self, hwnd: isize) {
        self.actions.forget(hwnd);
        let removed = {
            let mut adapters = self.adapters.borrow_mut();
            adapters
                .iter()
                .position(|&(known, _)| known == hwnd)
                .map(|index| adapters.remove(index))
        };
        drop(removed);
    }

    /// Answers a `WM_GETOBJECT` for `hwnd`.
    pub(super) fn get_object(&self, hwnd: isize, w_param: Wparam, l_param: Lparam) -> GetObject {
        let mut activation = Activation::default();
        let provider = {
            let mut adapters = self.adapters.borrow_mut();
            adapters
                .iter_mut()
                .find(|(known, _)| *known == hwnd)
                .and_then(|(_, adapter)| {
                    adapter.handle_wm_getobject(WPARAM(w_param), LPARAM(l_param), &mut activation)
                })
        };
        // The borrow is gone: handing the provider to UI Automation is what
        // may send the nested `WM_GETOBJECT`.
        let lresult = provider.map(|provider| {
            let LRESULT(lresult) = provider.into();
            lresult
        });
        GetObject {
            lresult,
            activated: activation.asked,
        }
    }

    /// Tells `hwnd`'s adapter whether the window has the keyboard focus.
    pub(super) fn focus(&self, hwnd: isize, focused: bool) {
        let events = {
            let mut adapters = self.adapters.borrow_mut();
            adapters
                .iter_mut()
                .find(|(known, _)| *known == hwnd)
                .and_then(|(_, adapter)| adapter.update_window_focus_state(focused))
        };
        if let Some(events) = events {
            events.raise();
        }
    }

    /// Hands `update` to `hwnd`'s adapter, which keeps it only while a reader
    /// is active, and raises what it changed.
    pub(super) fn update(&self, hwnd: isize, update: TreeUpdate) {
        let events = {
            let mut adapters = self.adapters.borrow_mut();
            adapters
                .iter_mut()
                .find(|(known, _)| *known == hwnd)
                .and_then(|(_, adapter)| adapter.update_if_active(|| update))
        };
        if let Some(events) = events {
            events.raise();
        }
    }
}
