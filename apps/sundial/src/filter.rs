//! The shadow knobs, as this sample reaches them: the console's own variables,
//! read and written by name — which filter runs, where the comparison seam
//! stands, and the sun's two bias counts.
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

/// The sun's constant shadow bias, in texels of the cascade a fragment landed
/// in.
///
/// One of `docs/plan/45-shadows.md`'s seventh decision's pair, and the half that
/// moves the compared depth **towards the light**: too little of it draws acne,
/// too much lifts a shadow off the thing casting it.
pub const BIAS: &str = "r_shadow_bias";

/// How far along its own normal a receiver moves before the sun's shadow lookup,
/// in the same texels.
///
/// The seventh decision's other half, and the one that moves the receiver
/// **sideways** rather than light-ward. Two variables and not one quality knob,
/// for that decision's own reason: the two are pulled in different directions by
/// the same pair of artefacts, and a single count could not show them moving
/// against each other.
pub const OFFSET: &str = "r_shadow_normal_offset";

/// Every variable this sample drives, in the order the panel prints them.
///
/// One list rather than two call sites, because two things walk it: the panel
/// and [`reset`], and a variable added to one and forgotten by the other is a
/// knob a run cannot put back.
pub const KNOBS: [&str; 4] = [FILTER, SPLIT, BIAS, OFFSET];

/// Where the seam stands when it is switched on.
///
/// The middle of the frame, which is what a comparison wants: the two filters
/// over the same geometry, each with half the picture. [`nudge_seam`] is how it
/// moves off centre.
pub const SEAM_CENTRE: f32 = 0.5;

/// How far one press moves the seam.
const SEAM_STEP: f32 = 0.02;

/// How far one press moves either bias count, in cascade texels.
///
/// **One step for both**, because both are denominated in the same texel: a
/// press means the same amount of bias whichever of the two it is spent on,
/// which is what makes walking one against the other a comparison.
///
/// **Half a texel, which is the step the two constants' own sweeps are written
/// in** — `crcbl_render::shadow`'s `DEPTH_BIAS_TEXELS` and `NORMAL_OFFSET_TEXELS`
/// each carry a table walked in halves — and both shipped values are multiples
/// of it, so a walk comes back to what ships rather than past it. It does **not**
/// reach the far end of either range in any reasonable number of presses; that
/// end is where `apps/sundial`'s plinth loses its shadow, it is dozens of texels
/// out, and a console line is how a person goes there. A step coarse enough to
/// walk that far would be too coarse to stand on the shipped count, which is
/// where a comparison starts.
const BIAS_STEP: f32 = 0.5;

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

/// Moves one bias count a `BIAS_STEP` up or down, inside its own range.
///
/// **[`BIAS`] and [`OFFSET`] through one function**, because a count walked by a
/// key is one fact about a control and the two cells differ only in which is
/// written. Clamped into the variable's own declared range, on [`nudge_seam`]'s
/// terms: a press at either end leaves the count there rather than being refused
/// by the console.
pub fn nudge_bias(name: &str, up: bool) {
    let Some((min, max)) = float_range(name) else {
        return;
    };
    let step = if up { BIAS_STEP } else { -BIAS_STEP };
    set(
        name,
        &Value::Float((var(name).get_f32() + step).clamp(min, max)),
    );
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
    /// The sun's constant shadow bias, in **thousandths of a cascade texel**.
    ///
    /// Thousandths for [`Knobs::seam_permille`]' reason and no other: a reading
    /// is compared against the last one to decide whether the panel is rebuilt,
    /// and that comparison wants `Eq`. `BIAS_STEP` is half a texel, so a
    /// thousandth resolves every value a key or a console line can leave here.
    pub bias_millitexels: u32,
    /// How far along its own normal a receiver moves, in the same thousandths.
    pub offset_millitexels: u32,
}

/// A count of thousandths of a texel, back as the texels the console holds.
fn texels(millitexels: u32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a bias count is a few thousand thousandths"
    )]
    {
        millitexels as f32 / 1000.0
    }
}

/// A count of texels as the thousandths a [`Knobs`] keeps.
fn millitexels(count: f32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both variables declare a range above zero, at most a few hundred texels"
    )]
    {
        (count * 1000.0).round() as u32
    }
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
            bias_millitexels: millitexels(var(BIAS).get_f32()),
            offset_millitexels: millitexels(var(OFFSET).get_f32()),
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

    /// The sun's constant bias, back as the texels the console holds.
    #[must_use]
    pub fn bias(self) -> f32 {
        texels(self.bias_millitexels)
    }

    /// The sun's normal offset, in the same texels.
    #[must_use]
    pub fn offset(self) -> f32 {
        texels(self.offset_millitexels)
    }

    /// What a bias row prints: a count and the unit it is denominated in.
    ///
    /// **The unit is on the row**, because a bare `1.50` beside a seam printed as
    /// a fraction of the width is two numbers a reviewer has no reason to read
    /// differently — and these are texels of whichever cascade the fragment
    /// landed in rather than metres, which is the whole of why the pair is
    /// comparable across one frame at all.
    #[must_use]
    pub fn bias_row(count: f32) -> String {
        format!("{count:.2} texels")
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
        section.row_str("bias", &Self::bias_row(self.bias()));
        section.row_str("normal offset", &Self::bias_row(self.offset()));
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
        assert_eq!(
            KNOBS.len(),
            4,
            "the panel prints one row per knob: the filter, the seam and the two bias counts"
        );
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
        for name in [BIAS, OFFSET] {
            let Kind::Float { min, max } = var(name).kind() else {
                panic!("{name} is a count of texels, so it is a float");
            };
            assert!(
                min <= 0.0,
                "{name} cannot reach zero, and zero is the end of its range this sample's \
                 acne reading is taken at"
            );
            let Value::Float(shipped) = *var(name).default() else {
                panic!("{name}'s default is the float the engine ships");
            };
            assert!(
                max > shipped * 2.0,
                "{name} ships at {shipped} and stops at {max}, so a walk cannot reach the far \
                 side of what the engine draws with"
            );
        }
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

    /// **Each bias count walks a step at a time and stops at its own ends**, and
    /// walking one leaves the other where it was.
    ///
    /// The last clause is the half worth guarding: [`nudge_bias`] takes the cell
    /// by name, so a key table that handed it the wrong one would move a knob
    /// while the panel row beside it said the other had moved.
    #[test]
    fn each_bias_count_walks_a_step_at_a_time_and_stops_at_its_own_ends() {
        let _held = held();
        for name in [BIAS, OFFSET] {
            let other = if name == BIAS { OFFSET } else { BIAS };
            let (min, max) = float_range(name).expect("a bias count is a float");
            let start = var(name).get_f32();
            let untouched = var(other).get_f32();

            nudge_bias(name, true);
            assert!(
                (var(name).get_f32() - (start + BIAS_STEP)).abs() < 1e-5,
                "{name} went to {} rather than a step up from {start}",
                var(name).get_f32()
            );
            nudge_bias(name, false);
            assert!((var(name).get_f32() - start).abs() < 1e-5);
            assert!(
                (var(other).get_f32() - untouched).abs() < 1e-5,
                "walking {name} moved {other} as well"
            );

            // Far more presses than either range is steps wide, so the walk
            // arrives at the end rather than merely near it.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a range of a few hundred texels over a half-texel step is a small count"
            )]
            let presses = ((max - min) / BIAS_STEP) as u32 + 2;
            for _ in 0..presses {
                nudge_bias(name, false);
            }
            assert!(
                (var(name).get_f32() - min).abs() < 1e-5,
                "{name} stopped at {} rather than at its own floor {min}",
                var(name).get_f32()
            );
            for _ in 0..presses {
                nudge_bias(name, true);
            }
            assert!(
                (var(name).get_f32() - max).abs() < 1e-5,
                "{name} stopped at {} rather than at its own ceiling {max}",
                var(name).get_f32()
            );
            reset();
        }
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
        nudge_bias(BIAS, true);
        nudge_bias(OFFSET, false);
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
