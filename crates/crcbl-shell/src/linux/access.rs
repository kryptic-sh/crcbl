//! Accessibility on Wayland and X11: each window's tree served over AT-SPI by
//! `accesskit_unix`. Behind the `accesskit` feature.
//!
//! # One adapter per window, driven from the shell
//!
//! AT-SPI is a D-Bus protocol, not a window-system one, so both backends use
//! this module the same way: an [`Adapter`] made with the window and dropped
//! with it, told when the window's focus changes, and handed each tree the
//! program sends through
//! [`Shell::update_accessibility`](crate::Shell::update_accessibility). The
//! adapter registers with the accessibility bus itself, on an executor thread
//! of its own.
//!
//! # Everything a reader does arrives on that other thread
//!
//! `accesskit_unix` calls all three handlers off the shell's thread: when the
//! bus says assistive technologies are enabled (activation), when a reader asks
//! for an action, and when the last one goes away (deactivation). Each becomes
//! a [`Raised`] on a mutex-guarded queue the shell drains in its pump, and the
//! handler rings the shell's [`ShellWaker`] so a loop asleep in
//! [`wait_events`](crate::Shell::wait_events) hears it now.
//!
//! # The tree is the program's
//!
//! Activation offers no tree: the program hears
//! [`AccessibilityActivated`](ShellEvent::AccessibilityActivated) and sends a
//! whole one, which `update_if_active` takes as the initial tree. Unlike
//! Windows, the bus does say when the last reader stopped, so
//! [`AccessibilityDeactivated`](ShellEvent::AccessibilityDeactivated) is
//! raised here.

use std::sync::{Arc, Mutex, PoisonError};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_unix::Adapter;

use crate::{ShellEvent, ShellWaker, WindowId};

/// What a handler raised, kept small and `Send` until the pump turns it into a
/// [`ShellEvent`].
#[derive(Debug)]
enum Raised {
    Activated,
    Action(ActionRequest),
    Deactivated,
}

/// What readers raised, by window, waiting for the pump. Shared with every
/// handler, which runs on the adapter's thread.
#[derive(Clone, Debug, Default)]
struct Queue(Arc<Mutex<Vec<(WindowId, Raised)>>>);

impl Queue {
    fn push(&self, window: WindowId, raised: Raised) {
        // A poisoned lock means a panic elsewhere while pushing or taking, and
        // the vector is still whole: no entry is half-written.
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((window, raised));
    }

    fn take(&self) -> Vec<(WindowId, Raised)> {
        core::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn forget(&self, window: WindowId) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|&(known, _)| known != window);
    }
}

/// One window's way back to the shell: queue what happened, ring the doorbell.
#[derive(Clone)]
struct Handler {
    window: WindowId,
    queue: Queue,
    waker: Option<ShellWaker>,
}

impl Handler {
    fn raise(&self, raised: Raised) {
        self.queue.push(self.window, raised);
        if let Some(waker) = &self.waker {
            waker.wake();
        }
    }
}

impl ActivationHandler for Handler {
    /// `None`: the program sends the first tree once it hears the activation.
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.raise(Raised::Activated);
        None
    }
}

impl ActionHandler for Handler {
    fn do_action(&mut self, request: ActionRequest) {
        self.raise(Raised::Action(request));
    }
}

impl DeactivationHandler for Handler {
    fn deactivate_accessibility(&mut self) {
        self.raise(Raised::Deactivated);
    }
}

/// Every window's adapter and the queue their readers fill. See the
/// [module docs](self).
#[derive(Debug, Default)]
pub(crate) struct Adapters {
    adapters: Vec<(WindowId, Adapter)>,
    queue: Queue,
}

impl Adapters {
    /// Gives `window` an adapter, whose handlers ring `waker` once they have
    /// queued what a reader did.
    pub(crate) fn open(&mut self, window: WindowId, waker: Option<ShellWaker>) {
        let handler = Handler {
            window,
            queue: self.queue.clone(),
            waker,
        };
        let adapter = Adapter::new(handler.clone(), handler.clone(), handler);
        self.adapters.push((window, adapter));
    }

    /// Drops `window`'s adapter, which unregisters it from the bus, and what
    /// its readers raised that the pump has not delivered.
    pub(crate) fn close(&mut self, window: WindowId) {
        self.adapters.retain(|(known, _)| *known != window);
        self.queue.forget(window);
    }

    fn adapter(&mut self, window: WindowId) -> Option<&mut Adapter> {
        self.adapters
            .iter_mut()
            .find(|(known, _)| *known == window)
            .map(|(_, adapter)| adapter)
    }

    /// Tells `window`'s adapter whether the window has the keyboard focus.
    pub(crate) fn focus(&mut self, window: WindowId, focused: bool) {
        if let Some(adapter) = self.adapter(window) {
            adapter.update_window_focus_state(focused);
        }
    }

    /// Hands `update` to `window`'s adapter, which keeps it only while a
    /// reader is active.
    pub(crate) fn update(&mut self, window: WindowId, update: TreeUpdate) {
        if let Some(adapter) = self.adapter(window) {
            adapter.update_if_active(|| update);
        }
    }

    /// What readers raised since the last call, oldest first, as events.
    pub(crate) fn take_events(&self) -> impl Iterator<Item = ShellEvent> {
        self.queue
            .take()
            .into_iter()
            .map(|(window, raised)| match raised {
                Raised::Activated => ShellEvent::AccessibilityActivated { window },
                Raised::Action(request) => ShellEvent::AccessibilityAction { window, request },
                Raised::Deactivated => ShellEvent::AccessibilityDeactivated { window },
            })
    }
}
