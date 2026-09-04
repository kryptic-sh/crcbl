//! The shadow-filter knobs, as this sample reaches them: the console's own
//! variables, read and written by name.
//!
//! ```text
//!  key / pause row ──▶ filter::* ──▶ crcbl_render's `r_shadow_*` ConVar
//!                                       │  (the variable IS the storage)
//!  console line ──────────────────────▶ ┘
//! ```
//!
//! # Why by name
//!
//! `apps/alcove/src/occlusion.rs`' argument, and it holds here for a second
//! reason on top of it. Alcove reaches `crcbl_render::ssao` by name because that
//! module is private; `crcbl_render::shadow` is not, and this module could name
//! [`crcbl::render::shadow::r_shadow_filter`] directly. It goes through
//! [`crcbl::render::console_table`] anyway because **that is the seam a person
//! typing `r_shadow_filter box` goes through**, and a pause row and a typed line
//! cannot hold two answers that disagree if there is one cell and both write it.
//!
//! What is read off the module rather than off the table is
//! [`crcbl::render::shadow::shipped_filter`] — the far side of the seam. That one
//! is the engine's own statement of which rung ships, and a second copy of it in
//! a sample goes stale the day the default moves.
//!
//! # Nothing here is kept
//!
//! There is no state in this module. A [`Knobs`] is a *reading*, taken once a
//! frame for the panel and the summary; the values live in the console's cells
//! and in nothing else. The one thing this fixture does keep is its clock, and
//! that lives in [`crate::sun::Clock`] because a tick is not a setting.

use crcbl::console::{ConVar, Kind, Value};
use crcbl::render::shadow::{self, Filter};

/// Which filter the near side of the seam runs.
pub const FILTER: &str = "r_shadow_filter";

/// Where the comparison seam stands, as a fraction of the frame's width.
pub const SPLIT: &str = "r_shadow_split";

/// Every variable this sample drives, in the order the panel prints them.
///
/// One list rather than two call sites, because two things walk it: the panel
/// and [`reset`], and a variable added to one and forgotten by the other is a
/// knob a run cannot put back.
pub const KNOBS: [&str; 2] = [FILTER, SPLIT];

/// Where the seam stands when it is switched on.
///
/// The middle of the frame, which is what a comparison wants: the two filters
/// over the same geometry, each with half the picture. [`nudge_seam`] is how it
/// moves off centre.
pub const SEAM_CENTRE: f32 = 0.5;

/// How far one press moves the seam.
const SEAM_STEP: f32 = 0.02;

/// The variable `name` names, out of `crcbl-render`'s own console table.
///
/// # Panics
///
/// If the engine declares no such variable, which is a mistake in [`KNOBS`]
/// rather than a condition a run can be in —
/// `every_knob_this_sample_drives_is_declared_by_the_engine` is what catches it
/// with no GPU and no window.
#[must_use]
pub fn var(name: &str) -> &'static ConVar {
    crcbl::render::console_table()
        .vars()
        .iter()
        .copied()
        .find(|var| var.name() == name)
        .unwrap_or_else(|| panic!("crcbl-render declares no `{name}` variable"))
}

/// Writes `value` into `name`, and says so in the log if the console refused.
///
/// **Refusal is reported rather than dropped.** Every caller below clamps into
/// the variable's own [`Kind`] first, so a refusal here means the two disagree —
/// which is worth a line rather than a knob that silently did not move.
fn set(name: &str, value: &Value) {
    if let Err(fault) = var(name).set(value) {
        crcbl::log::error!("sundial: the console refused {name} = {value}: {fault}");
    }
}

/// The names a [`Kind::Enum`] variable accepts, in the order it declares them.
///
/// Empty for any other kind, which no caller here has: [`FILTER`] is the one
/// enum this sample drives and `the_filter_row_cycles_the_engines_own_set` is
/// what holds that.
#[must_use]
pub fn names(name: &str) -> &'static [&'static str] {
    match var(name).kind() {
        Kind::Enum(names) => names,
        _ => &[],
    }
}

/// Moves an enum variable on to the next name it declares, wrapping.
pub fn cycle(name: &str) {
    let names = names(name);
    if names.is_empty() {
        return;
    }
    let current = var(name).get_enum();
    let at = names
        .iter()
        .position(|entry| *entry == current)
        .unwrap_or(0);
    set(name, &Value::Enum(names[(at + 1) % names.len()]));
}

/// The inclusive float range `name` accepts, or `None` for another kind.
fn float_range(name: &str) -> Option<(f32, f32)> {
    match var(name).kind() {
        Kind::Float { min, max } => Some((min, max)),
        _ => None,
    }
}

/// Puts the seam at [`SEAM_CENTRE`], or takes it away if it is already up.
pub fn toggle_seam() {
    let at = if seam().is_some() { 0.0 } else { SEAM_CENTRE };
    set(SPLIT, &Value::Float(at));
}

/// Moves the seam one step left or right, within the variable's own range.
///
/// A no-op while the seam is down: nudging a comparison that is not being drawn
/// would leave the variable somewhere a later press did not ask for.
pub fn nudge_seam(right: bool) {
    let Some(at) = seam() else {
        return;
    };
    let Some((min, max)) = float_range(SPLIT) else {
        return;
    };
    let step = if right { SEAM_STEP } else { -SEAM_STEP };
    set(SPLIT, &Value::Float((at + step).clamp(min, max)));
}

/// Puts the seam at `at`, the same fraction of the frame's width the variable
/// itself is in, and answers with where it stands afterwards.
///
/// **Zero and one take it down**, which is [`seam`]'s rule rather than a second
/// one. `at` is clamped into the variable's own range, so a caller that sends a
/// number from outside it moves the seam to the nearest edge rather than being
/// refused by the console.
pub fn set_seam(at: f32) -> Option<f32> {
    let Some((min, max)) = float_range(SPLIT) else {
        return seam();
    };
    set(SPLIT, &Value::Float(at.clamp(min, max)));
    seam()
}

/// Where the seam stands, or `None` for a frame comparing nothing.
///
/// **Zero and one are both "off"**, which is
/// [`crcbl::render::shadow::split_at`]'s own rule: a seam at either edge leaves
/// one side of the frame empty, so every fragment takes one lane. Read through
/// the engine's own function rather than re-derived, so the panel and the shader
/// cannot disagree about what a person is looking at.
#[must_use]
pub fn seam() -> Option<f32> {
    shadow::split_at()
}

/// Puts every knob in [`KNOBS`] back to the value the engine declares.
///
/// What `--filter`, `--split` and the keys have moved, undone — which is what a
/// golden run needs between two frames it means to compare, and what the binary
/// does on its way out so a console left mid-experiment does not reach the next
/// process through a settings file.
pub fn reset() {
    for name in KNOBS {
        let var = var(name);
        let default = var.default().clone();
        set(name, &default);
    }
}

/// What every knob reads right now.
///
/// A reading rather than a store — see this module's header — taken once per
/// frame by `crate::app` so the panel, the pause menu's labels and the headless
/// summary all report the same instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Knobs {
    /// The filter the near side of the seam runs.
    pub filter: Filter,
    /// The filter the far side runs: the one the engine **ships**, read off the
    /// variable's own declared default by
    /// [`crcbl::render::shadow::shipped_filter`] rather than written down here.
    pub shipped: Filter,
    /// Where the seam stands, as a fraction of the frame's width in thousandths,
    /// or `None` for a frame comparing nothing.
    ///
    /// Thousandths rather than an `f32` so a [`Knobs`] is `Eq`: `crate::app`
    /// rebuilds the pause panel when this value changes, and comparing the
    /// reading is how it knows. The seam's own step is a fiftieth of the width,
    /// so a thousandth resolves every position a key or a page can ask for.
    pub seam_permille: Option<u32>,
}

impl Knobs {
    /// Reads every knob.
    #[must_use]
    pub fn read() -> Self {
        Self {
            filter: shadow::filter(),
            shipped: shadow::shipped_filter(),
            seam_permille: seam().map(|at| {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "`seam` answers inside 0..1, so the product is inside 0..1000"
                )]
                {
                    (at * 1000.0).round() as u32
                }
            }),
        }
    }

    /// Where the seam stands, back as the fraction the console holds.
    #[must_use]
    pub fn seam(self) -> Option<f32> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a value under a thousand is exact in an f32"
        )]
        self.seam_permille.map(|permille| permille as f32 / 1000.0)
    }

    /// What the panel and the summary call the near side of the seam.
    #[must_use]
    pub fn near_side(self) -> String {
        format!("{} (console)", self.filter.label())
    }

    /// The same for the far side, which is only a side at all while the seam is
    /// up.
    ///
    /// **This row is what `docs/plan/sample/18-sundial.md`'s Scope asks the
    /// sample for.** The engine half puts a different filter on each side of the
    /// seam; the sample's half is saying which, because two shadowed pictures
    /// side by side name neither.
    #[must_use]
    pub fn far_side(self) -> String {
        match self.seam_permille {
            Some(_) => format!("{} (shipped)", self.shipped.label()),
            None => "no seam: one filter".to_string(),
        }
    }

    /// Where the seam is, in words.
    #[must_use]
    pub fn seam_row(self) -> String {
        match self.seam() {
            Some(at) => format!("{at:.2} of the width"),
            None => "OFF".to_string(),
        }
    }
}

impl crcbl::ui::DebugModule for Knobs {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("shadow filter");
        section.row_str("filter", self.filter.label());
        section.row_str("seam", &self.seam_row());
        section.row_str("near side", &self.near_side());
        section.row_str("far side", &self.far_side());
    }
}

/// The filter `name` names, **as the engine spells it**.
///
/// Matched against [`FILTER`]'s own declared set rather than against a list
/// here, which is what makes a fourth rung landing in `crcbl-render` a value
/// `--filter` takes without a change in this file.
#[must_use]
pub fn filter_from_name(name: &str) -> Option<&'static str> {
    names(FILTER)
        .iter()
        .copied()
        .find(|declared| *declared == name)
}

/// A seam position by the name `--split` takes, or `None` for anything that is
/// not a fraction inside the variable's own range.
#[must_use]
pub fn seam_from_name(name: &str) -> Option<f32> {
    let at: f32 = name.parse().ok()?;
    match var(SPLIT).kind() {
        Kind::Float { min, max } => (at >= min && at <= max).then_some(at),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises every check that moves a knob, and puts them all back.
    ///
    /// **Every check here takes it, and that is the point.** A [`ConVar`] is
    /// process-global by design and `cargo test` runs a crate's tests as threads
    /// of one process, so two checks that move the filter are two writers to one
    /// cell — which shows up as a flake rather than as a failure anybody can
    /// read.
    struct Held {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            reset();
        }
    }

    /// The lock [`Held`] takes.
    static KNOB_SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn held() -> Held {
        let guard = KNOB_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset();
        Held { _guard: guard }
    }

    /// **Every name this sample drives is one the engine declares**, and each is
    /// the kind the code above assumes.
    ///
    /// The check that stops [`var`]'s panic being something a person meets at
    /// run time: a variable renamed in `crcbl-render` fails here with no GPU
    /// rather than on the first press of a key.
    #[test]
    fn every_knob_this_sample_drives_is_declared_by_the_engine() {
        let _held = held();
        assert_eq!(KNOBS.len(), 2, "the panel prints one row per knob");
        for name in KNOBS {
            let var = var(name);
            assert_eq!(var.name(), name);
            assert!(!var.help().is_empty(), "{name} has no help line");
        }
        assert!(
            matches!(var(FILTER).kind(), Kind::Enum(_)),
            "the filter is a name from a set"
        );
        assert!(matches!(var(SPLIT).kind(), Kind::Float { .. }));
    }

    /// **Cycling the filter walks the engine's own set**, and comes back round
    /// to where it started.
    ///
    /// Written against [`names`] rather than against three literals, so a fourth
    /// rung landing in `crcbl-render` is one this sample reaches without a change
    /// here — and a set that shrank to one is caught by the assertion that it has
    /// more than one member, which is what makes the seam a comparison at all.
    #[test]
    fn the_filter_row_cycles_the_engines_own_set() {
        let _held = held();
        let names = names(FILTER);
        assert!(
            names.len() > 1,
            "a seam between one filter and itself compares nothing"
        );
        let start = var(FILTER).get_enum();
        let mut seen = vec![start];
        for _ in 1..names.len() {
            cycle(FILTER);
            let now = var(FILTER).get_enum();
            assert!(!seen.contains(&now), "cycling repeated {now} early");
            seen.push(now);
        }
        cycle(FILTER);
        assert_eq!(var(FILTER).get_enum(), start, "the cycle must wrap");
        seen.sort_unstable();
        let mut declared = names.to_vec();
        declared.sort_unstable();
        assert_eq!(seen, declared, "cycling must reach every declared name");
    }

    /// **Every name the console declares is a [`Filter`] with the same label**,
    /// so the reading the panel prints is the rung the shader ran.
    ///
    /// The failure this catches is a name added to the `convar!` and not to
    /// [`Filter`]: `Filter::of` answers `Pcss` for a name it does not know, so
    /// the panel would say `pcss` while the console said something else and the
    /// picture would agree with neither row.
    #[test]
    fn every_declared_name_is_a_filter_that_labels_itself_the_same_way() {
        let _held = held();
        for name in names(FILTER) {
            set(FILTER, &Value::Enum(name));
            let read = Knobs::read();
            assert_eq!(
                read.filter.label(),
                *name,
                "the console holds {name} and the panel would print {}",
                read.filter.label()
            );
            assert_eq!(filter_from_name(name), Some(*name));
        }
        assert_eq!(filter_from_name("poisson"), None);
    }

    /// **The seam goes up at the centre, moves, and comes back down**, and a
    /// nudge with the seam down changes nothing.
    ///
    /// The last clause is the one worth guarding: a nudge that wrote a value
    /// while the seam was off would switch the comparison on from a key that says
    /// it moves one.
    #[test]
    fn the_seam_goes_up_at_the_centre_and_moves_only_while_it_is_up() {
        let _held = held();
        assert_eq!(seam(), None, "a fresh run compares nothing");
        nudge_seam(true);
        assert_eq!(seam(), None, "a nudge must not raise the seam");

        toggle_seam();
        assert_eq!(seam(), Some(SEAM_CENTRE));
        nudge_seam(true);
        let moved = seam().expect("the seam is up");
        assert!(
            (moved - (SEAM_CENTRE + SEAM_STEP)).abs() < 1e-5,
            "the seam went to {moved}"
        );
        nudge_seam(false);
        assert!((seam().expect("still up") - SEAM_CENTRE).abs() < 1e-5);

        toggle_seam();
        assert_eq!(seam(), None, "a second press takes the comparison away");
    }

    /// **A caller can place the seam outright**, and either edge takes it down.
    #[test]
    fn the_seam_can_be_placed_outright_and_either_edge_takes_it_down() {
        let _held = held();
        assert_eq!(set_seam(0.25), Some(0.25));
        assert_eq!(seam(), Some(0.25));
        assert_eq!(set_seam(0.0), None, "the left edge is no comparison");
        assert_eq!(set_seam(0.75), Some(0.75));
        assert_eq!(set_seam(1.0), None, "the right edge is no comparison");
        assert_eq!(set_seam(7.0), None, "past the range is the range's own end");
        assert!(
            (var(SPLIT).get_f32() - 1.0).abs() < 1e-6,
            "and it stopped there"
        );
        assert_eq!(seam_from_name("0.25"), Some(0.25));
        assert_eq!(seam_from_name("7"), None);
        assert_eq!(seam_from_name("sideways"), None);
    }

    /// **The panel says which filter each side of the seam is running**, and says
    /// there is no far side when there is no seam.
    #[test]
    fn the_panel_names_the_filter_on_each_side_of_the_seam() {
        let _held = held();
        let shipped = shadow::shipped_filter();

        let alone = Knobs::read();
        assert!(alone.far_side().contains("no seam"), "{}", alone.far_side());
        assert_eq!(alone.seam_row(), "OFF");

        // Move the console's filter off the shipped one, which is the state the
        // seam exists to draw.
        cycle(FILTER);
        toggle_seam();
        let compared = Knobs::read();
        assert_ne!(
            compared.filter, compared.shipped,
            "a seam between one filter and itself is not a comparison"
        );
        assert!(
            compared.near_side().contains(compared.filter.label()),
            "the near row does not name the filter it runs: {}",
            compared.near_side()
        );
        assert!(
            compared.far_side().contains(shipped.label()),
            "the far row does not name the shipped filter: {}",
            compared.far_side()
        );
        assert!(
            compared.seam_row().contains("0.50"),
            "the seam row does not say where it stands: {}",
            compared.seam_row()
        );
        assert_eq!(compared.seam(), Some(SEAM_CENTRE));
    }

    /// **`reset` puts every knob back**, which is what a run that has been
    /// experimented on owes the next frame it means to compare.
    #[test]
    fn reset_puts_every_knob_back_to_the_engines_default() {
        let _held = held();
        let before: Vec<Value> = KNOBS.iter().map(|name| var(name).get()).collect();
        cycle(FILTER);
        toggle_seam();
        let moved: Vec<Value> = KNOBS.iter().map(|name| var(name).get()).collect();
        assert_ne!(before, moved, "nothing moved, so there is nothing to reset");

        reset();
        let after: Vec<Value> = KNOBS.iter().map(|name| var(name).get()).collect();
        assert_eq!(after, before);
        for name in KNOBS {
            assert_eq!(&var(name).get(), var(name).default(), "{name}");
        }
        assert_eq!(
            Knobs::read().filter,
            shadow::shipped_filter(),
            "a reset run draws the filter that ships"
        );
    }
}
