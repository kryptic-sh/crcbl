//! The debug view every game shares: one console variable, one application.
//!
//! `docs/plan/52-debug-console.md` decision 8 — "the AO view, and every debug
//! view, in every build". [`crcbl_render::ForwardRenderer`] has carried five
//! independent switches and a precedence resolver for a long time, and until
//! this module nothing engine-level set them: `apps/lantern` hand-wired the
//! occlusion view behind its pause panel, `apps/quarry` owned a
//! [`DebugView`] of its own, `apps/viewer` bound `N` to the normals view, and
//! the other thirteen samples had no way to reach any of it.
//!
//! ```text
//! `debug_view ambient occlusion` ─┐
//! lantern's AO VIEW row ──────────┼─→ r_debug_view ─→ Loop::apply_debug_view
//! quarry's LOD/HEATMAP rows ──────┤                     └─→ GameGpu::set_debug_view
//! viewer's N key ─────────────────┘
//! ```
//!
//! # Why the variable is the storage, and why it lives here
//!
//! A [`ConVar`](crcbl_console::ConVar) **is** the value, Source-style — plan
//! decision 1 — so a row and a console line cannot hold two answers that
//! disagree. That is the whole reason the three samples above gave up their own
//! fields: each wrote its view into the renderer on every frame, so whichever of
//! the two ran last won, and a console line was silently undone by the next
//! frame.
//!
//! It is declared in `crcbl` rather than in `crcbl-render`, where decision 8
//! sketched it, because **nothing in `crcbl-render` can apply it**: a static has
//! no renderer to reach, and the seam that does is
//! [`GameGpu::set_debug_view`](crate::engine::GameGpu::set_debug_view), which is
//! this crate's. The variable and the code that acts on it are therefore in one
//! crate, which is what decision 2 asks of a declaration.
//!
//! # The cost of a process-global
//!
//! One `AtomicUsize` load a frame in [`Loop::frame`](crate::engine::Loop::frame)
//! and a write only where the value moved. What it costs a **test** is that two
//! loops in one process share it: every check that moves the view has to be a
//! process of its own, which is what `cargo nextest` gives and what CI runs, or
//! else hold [`for_test`] — which nests, so a check that builds two loops is
//! not a deadlock.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crcbl_console::Value;
use crcbl_render::DebugView;

crcbl_console::convar! {
    /// Which channel the frame draws instead of the shaded picture.
    ///
    /// `debug_view` is the command that sets it in words.
    pub static r_debug_view: &'static str one_of [
        "shaded",
        "heatmap",
        "lod tint",
        "normals",
        "ambient occlusion",
        "motion",
    ] = "shaded";
}

crcbl_console::concommand! {
    /// Draw a debug channel instead of the shaded frame — `debug_view ambient occlusion`.
    ///
    /// With no argument, prints the view in force. The spelling
    /// `docs/plan/52-debug-console.md` decision 7 promises; `r_debug_view` is
    /// the variable underneath, and both reach the same cell.
    pub fn debug_view(cx, args) {
        if args.is_empty() {
            cx.print(format!("debug_view = {}", current().label()));
            return Ok(());
        }
        // Joined, not taken one at a time: `ambient occlusion` and `lod tint`
        // are one value each, which is the same rule `Registry::run_statement`
        // applies to a variable's own set.
        let typed = args.join(" ");
        let value = r_debug_view.kind().parse(&typed)?;
        r_debug_view.set(&value)?;
        cx.print(format!("debug_view = {value}"));
        Ok(())
    }
}

/// Every view, in [`DebugView`]'s own declaration order.
///
/// The order [`r_debug_view`] declares its names in. The `convar!` needs
/// literals, so the names are written twice; `the_variable_names_are_the_
/// renderers_labels` is what holds the copy to [`DebugView::label`].
const VIEWS: [DebugView; 6] = [
    DebugView::Shaded,
    DebugView::Heatmap,
    DebugView::LodTint,
    DebugView::Normals,
    DebugView::AmbientOcclusion,
    DebugView::Motion,
];

/// Where `view` sits in [`VIEWS`].
///
/// A `match` over the whole enum rather than a search, for
/// [`crate::settings::debug_view_switches`]'s reason: a [`DebugView`] variant
/// added later fails to compile here instead of quietly becoming a view the
/// console cannot name. Nothing outside the checks below calls it — it exists to
/// be exhaustive — so it is compiled with them, and a new variant reddens
/// `cargo clippy --all-targets` and the test run rather than the library build.
#[cfg(test)]
const fn slot(view: DebugView) -> usize {
    match view {
        DebugView::Shaded => 0,
        DebugView::Heatmap => 1,
        DebugView::LodTint => 2,
        DebugView::Normals => 3,
        DebugView::AmbientOcclusion => 4,
        DebugView::Motion => 5,
    }
}

/// The view the frame is drawn as, as every host reads it.
#[must_use]
pub fn current() -> DebugView {
    let name = r_debug_view.get_enum();
    VIEWS
        .into_iter()
        .find(|view| view.label() == name)
        .expect("`r_debug_view`'s set is `DebugView::label` of every view")
}

/// Draw `view` from the next frame on, in whatever host is running.
///
/// The write half of [`current`], for a menu row or a key binding: a sample that
/// used to reach the renderer itself calls this instead, so its row and a
/// console line are one value and not two.
pub fn set(view: DebugView) {
    let value = Value::Enum(view.label());
    r_debug_view
        .set(&value)
        .expect("`r_debug_view` is writable and holds `DebugView::label` of every view");
}

/// Show `view`, or go back to the shaded frame if it is already showing.
///
/// What a row that names one view does — `apps/lantern`'s `AO VIEW`,
/// `apps/quarry`'s `LOD`/`HEATMAP`, `apps/viewer`'s `N` — so all three press the
/// same way: a second press on the row that is on takes the picture back rather
/// than leaving the reviewer to find `shaded`.
pub fn toggle(view: DebugView) {
    set(if current() == view {
        DebugView::Shaded
    } else {
        view
    });
}

/// Serialises the tests that move the view, and puts it back afterwards.
///
/// **Test support, in the public API deliberately** — `crcbl_core::log::capture`
/// is the same shape and there for the same reason. A
/// [`ConVar`](crcbl_console::ConVar) is process-global by design (see the module
/// docs), and `cargo test` runs a crate's tests as threads of *one* process: two
/// checks that move the view are then two writers to one cell, which shows up as
/// a flake rather than as a failure anybody can read. Every check in this
/// workspace that moves the view takes this first, so the plain runner agrees
/// with `cargo nextest`, which gives each test a process of its own and needs
/// none of it.
///
/// The guard puts the view back to [`DebugView::Shaded`] at **both** ends, so a
/// check neither starts on what the one before it was looking at nor leaves its
/// own behind — a poisoned lock is stepped over, because a panicking test has
/// nothing to corrupt here.
///
/// **Taking it twice on one thread is allowed**, and it has to be: a check that
/// builds two loops holds two fixtures, and the second binding does not drop the
/// first — `let mut engine = scripted(..);` twice in one scope is the obvious
/// thing to write, and against a plain mutex it is a deadlock rather than a
/// failure. So the count is per thread: the first call takes the lock, a nested
/// one only raises the depth, and the lock goes back when the outermost guard
/// drops. Another *thread* still waits, which is the whole point. The guard is
/// deliberately neither `Send` nor `Sync` — a guard moved to another thread
/// would decrement a count it never raised — so misuse fails to compile.
#[must_use]
pub fn for_test() -> ViewLock {
    HELD.with_borrow_mut(|held| match held {
        Some((depth, _)) => *depth += 1,
        None => {
            *held = Some((1, VIEW_LOCK.lock().unwrap_or_else(PoisonError::into_inner)));
        }
    });
    set(DebugView::Shaded);
    ViewLock {
        _not_send: PhantomData,
    }
}

/// What [`for_test`] hands back: the view is this thread's until it drops.
#[derive(Debug)]
pub struct ViewLock {
    /// Neither `Send` nor `Sync`: the lock and the depth live in [`HELD`], which
    /// is this thread's, so a guard that crossed threads would release a lock
    /// the other thread is holding.
    _not_send: PhantomData<*const ()>,
}

impl Drop for ViewLock {
    fn drop(&mut self) {
        // Before the lock itself goes, so the next holder starts on the shaded
        // frame rather than on whatever this test was looking at. A nested guard
        // resets it too: the check around it is between loops either way.
        set(DebugView::Shaded);
        HELD.with_borrow_mut(|held| match held {
            // The outermost guard, so the lock goes back to whoever is waiting.
            Some((1, _)) | None => *held = None,
            Some((depth, _)) => *depth -= 1,
        });
    }
}

thread_local! {
    /// How many [`ViewLock`]s this thread holds, and the lock the first took.
    static HELD: RefCell<Option<(usize, MutexGuard<'static, ()>)>> =
        const { RefCell::new(None) };
}

/// [`for_test`]'s lock.
static VIEW_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every name the console offers is a [`DebugView::label`]**, and every
    /// view has one — so the set a person types from and the set the renderer
    /// draws cannot drift.
    ///
    /// The list in the `convar!` has to be literals, which is the one place a
    /// name is written twice; this is what holds the copy to the original.
    #[test]
    fn the_variable_names_are_the_renderers_labels() {
        let crcbl_console::Kind::Enum(names) = r_debug_view.kind() else {
            panic!("`r_debug_view` is an enum variable");
        };
        let labels: Vec<&str> = VIEWS.into_iter().map(DebugView::label).collect();
        assert_eq!(names, labels.as_slice());
    }

    /// **[`VIEWS`] holds every view exactly once**, which is what makes
    /// [`slot`]'s exhaustive match a guard: a variant added to [`DebugView`]
    /// fails to compile there, and one added there and forgotten here is this
    /// assertion.
    #[test]
    fn every_view_has_a_slot_of_its_own() {
        for (index, view) in VIEWS.into_iter().enumerate() {
            assert_eq!(slot(view), index, "{view:?} is in the wrong slot");
        }
    }

    /// **A view set is the view read back**, through the atomic the console
    /// writes.
    #[test]
    fn a_view_written_is_the_view_read_back() {
        let _view = for_test();
        for view in VIEWS {
            set(view);
            assert_eq!(current(), view);
        }
    }

    /// **A row toggles to its view and back to the shaded frame**, and a row
    /// that names another view *replaces* what is showing.
    ///
    /// Every start × every row, because that is where a naive toggle goes wrong:
    /// pressing `HEATMAP` while the tint is showing must replace it rather than
    /// leave both set, and pressing the row that is already on must come back to
    /// the shaded picture rather than doing nothing. This is the exclusivity
    /// `apps/quarry`'s two overlay rows used to own — a panel that could read
    /// `ON` twice about one picture is a panel nobody can act on — and the two
    /// rows share it with `apps/lantern`'s and `apps/viewer`'s now.
    #[test]
    fn a_row_replaces_the_view_showing_and_switches_itself_off() {
        let _view = for_test();
        for start in VIEWS {
            for row in VIEWS.into_iter().filter(|view| *view != DebugView::Shaded) {
                set(start);
                toggle(row);
                assert_eq!(
                    current(),
                    if start == row { DebugView::Shaded } else { row },
                    "{row:?} pressed from {start:?}",
                );
                // And pressing the same row twice comes back to where it
                // started, which is what a reviewer flicking a row expects.
                toggle(row);
                assert_eq!(
                    current(),
                    if start == row { row } else { DebugView::Shaded },
                    "{row:?} pressed twice from {start:?}",
                );
            }
        }
    }

    /// **A second guard on one thread is not a deadlock**, which is what a
    /// check that builds two loops takes without meaning to.
    ///
    /// `let mut engine = scripted(..);` written twice in one scope does not drop
    /// the first fixture — the binding is shadowed, not ended — so the second
    /// `for_test` lands while the first still holds the lock. Against a plain
    /// mutex that is a test that never finishes, which arrives as a 240-second
    /// nextest timeout and not as a failure anybody can read; `apps/viewer`'s
    /// `the_turntable_turns_until_someone_takes_hold_and_then_never_again` is
    /// the check that found it. **This is the avoidance, not the reproduction**:
    /// it completes because the count nests, and a non-reentrant lock hangs it.
    ///
    /// It also holds the release to the *outermost* drop: the inner guard going
    /// must not hand the lock to another thread while this one is still driving
    /// a loop.
    #[test]
    fn a_second_guard_on_one_thread_nests_instead_of_waiting() {
        let outer = for_test();
        set(DebugView::Motion);
        let inner = for_test();
        assert_eq!(
            current(),
            DebugView::Shaded,
            "a guard hands its holder the shaded frame, nested or not"
        );

        set(DebugView::Normals);
        drop(inner);
        assert!(
            VIEW_LOCK.try_lock().is_err(),
            "the inner guard gave the lock back while the outer one still holds it"
        );

        drop(outer);
        // **This thread's slot, not the global lock.** `VIEW_LOCK.try_lock()`
        // would answer the same question and answer it wrongly under load: the
        // lock is released by the line above, so any other test thread parked in
        // `for_test` may hold it before the next statement runs, and a `try_lock`
        // here then reports a guard that leaked when what really happened is
        // that the release worked and somebody took their turn. Failing that
        // way needs a second thread wanting the lock, which is why it only ever
        // reddened a whole-workspace run.
        //
        // [`HELD`] is this thread's and cannot be raced. Emptying it is what
        // releases the lock — the `MutexGuard` lives in there — so a `None` here
        // *is* the release, observed at its source.
        assert!(
            HELD.with_borrow(Option::is_none),
            "the outermost guard dropped without giving the lock back"
        );
        assert_eq!(current(), DebugView::Shaded);
    }

    /// **The default is the shaded picture**, which is what every golden in the
    /// workspace is blessed in.
    ///
    /// Asserted on the declaration rather than on the cell, because a test that
    /// read the cell would be reading whatever another test in this process left
    /// there.
    #[test]
    fn the_default_is_the_shaded_frame() {
        assert_eq!(*r_debug_view.default(), Value::Enum("shaded"));
    }
}
