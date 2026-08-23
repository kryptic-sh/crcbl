//! The teardown the three [`OffscreenSetup`] suites share — not a test module,
//! and not a test target either: `tests/offscreen/` holds no `main.rs`, so Cargo
//! compiles nothing here on its own.
//!
//! **Three suites pull this in with `#[path]`** — `tests/render_e2e.rs`,
//! `tests/tiling_e2e.rs` and `tests/gltf_e2e.rs` — because all three open a
//! device through [`OffscreenSetup`], draw frames on a real driver and then have
//! to ask that device whether the frames were legal. Each names itself in a
//! `SUITE` constant at its own crate root, which is what every line this file
//! prints is built from; there is nothing else suite-specific in here.
//!
//! # Why the suites needed this
//!
//! They were the three lavapipe CI steps a device error could not fail. Each one
//! ended its frame with `setup.finish().expect("the device reaches idle")`,
//! which reports a device lost and nothing else — so a frame the validation
//! layer had already refused was compared against its golden, matched, and
//! exited 0. A green run said the pixels agreed with a reference, not that the
//! commands that produced them were legal.
//!
//! [`OffscreenSetup::finish`] is where the answer arrives: it waits the device
//! idle and tears the frame's objects down in the order `crcbl-hal`'s obligation
//! 2 requires, so by the time it returns, everything the layer has to say about
//! this device — the frames *and* the teardown — has been said. [`Offscreen`] is
//! the wrapper that stops the return value being thrown away.
//!
//! # The Drop is half of it
//!
//! `finish` is a line in a test, and a test that panics before reaching it never
//! asks — which is exactly the run where the device's verdict is worth having.
//! So the teardown lives in [`Offscreen`]'s [`Drop`] and [`Offscreen::finish`]
//! is the explicit way to reach it: a fixture dropped by a panicking test still
//! tears down and still prints what it saw, and one dropped by a test that
//! simply forgot to call `finish` still fails the run rather than passing
//! quietly. [`OffscreenSetup`] has no `Drop` of its own, so a path that skipped
//! this leaked the device as well as the answer.
//!
//! **Nothing on the panicking path may panic.** It runs while the thread is
//! already unwinding, and a second panic aborts the process, destroying the
//! output this exists to produce.

use core::ops::{Deref, DerefMut};

use crcbl::screenshot::OffscreenSetup;

/// An [`OffscreenSetup`] whose teardown is asserted on rather than discarded.
///
/// [`Deref`] is what keeps every `setup.draw_and_readback()`, `setup.format()`
/// and `assert_pins_arrived(&setup)` in the suites reading as it did before the
/// wrapper existed.
pub(crate) struct Offscreen {
    /// Emptied by whichever of [`Offscreen::finish`] and [`Drop`] gets there
    /// first — they are the same path — so the teardown runs exactly once and
    /// every later deref panics with the message below rather than reaching a
    /// setup that has already given its device back.
    setup: Option<OffscreenSetup>,
    /// The suite's own name, for the lines this prints. `crate::SUITE`.
    suite: &'static str,
}

impl Offscreen {
    /// Takes ownership of an opened `setup` so its teardown cannot be dropped on
    /// the floor.
    pub(crate) fn guard(suite: &'static str, setup: OffscreenSetup) -> Self {
        Self {
            setup: Some(setup),
            suite,
        }
    }

    /// Tears down, and fails the test on anything the device reports.
    ///
    /// **Callers should end their frame with this rather than letting the
    /// fixture fall out of scope**, not because the teardown would be missed —
    /// [`Drop`] runs it either way — but because it fixes *when* it happens. The
    /// suites call it before their first assertion about the pixels on purpose:
    /// a device that never finished drawing, or a frame the layer refused, is
    /// the real answer, and a golden comparison that panicked first would report
    /// a wrong picture instead of it.
    pub(crate) fn finish(self) {
        // `Drop` is the whole of it, and deliberately the only copy: a second
        // teardown written out here is a second message to keep in step with the
        // one the panicking path prints.
        drop(self);
    }
}

impl Deref for Offscreen {
    type Target = OffscreenSetup;

    fn deref(&self) -> &Self::Target {
        self.setup
            .as_ref()
            .expect("this fixture has already been finished")
    }
}

impl DerefMut for Offscreen {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.setup
            .as_mut()
            .expect("this fixture has already been finished")
    }
}

impl Drop for Offscreen {
    fn drop(&mut self) {
        let Some(setup) = self.setup.take() else {
            return;
        };
        let suite = self.suite;
        // Before anything is decided about the answer: `OffscreenSetup` has no
        // `Drop`, so an early return above this line would leak the device along
        // with the frame's objects.
        let verdict = setup.finish();
        if !std::thread::panicking() {
            if let Err(why) = verdict {
                // A fresh panic rather than a second one — the thread is not
                // unwinding — so a test that forgot `finish` is still failed by
                // what the device said instead of passing quietly.
                panic!(
                    "{suite}: the device did not come through this frame cleanly: {why}. Every \
                     call in this test returned success, so this is a failure the return values \
                     did not carry."
                );
            }
            return;
        }
        // The panicking path: what `finish` would have said, on the runs that
        // never reach an assertion about it. A device lost during the frame and
        // a specification violation the driver accepted are both invisible from
        // a failing golden comparison, and both make every other symptom in the
        // report downstream noise.
        match verdict {
            Ok(()) => eprintln!(
                "{suite}: the fixture was torn down by a panicking test — the device reached \
                 idle and reported nothing, so the failure is in what it produced"
            ),
            Err(why) => eprintln!(
                "{suite}: the fixture was torn down by a panicking test, and the device did not \
                 come through the frame cleanly: {why}"
            ),
        }
    }
}
