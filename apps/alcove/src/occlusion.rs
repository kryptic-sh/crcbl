//! The occlusion knobs, as this sample reaches them: the console's own
//! variables, read and written by name.
//!
//! ```text
//!  key / pause row ──▶ occlusion::* ──▶ crcbl_render's `r_ssao_*` ConVar
//!                                          │  (the variable IS the storage)
//!  console line ─────────────────────────▶ ┘
//! ```
//!
//! # Why by name and not by symbol
//!
//! `crcbl_render::ssao` is a private module: the variables it declares reach the
//! outside world through [`crcbl::render::console_table`], which is the seam
//! `docs/plan/52-debug-console.md` decision 2 puts on a crate. So this module
//! looks each one up in that table rather than naming a `pub static` that does
//! not exist — and that is the right shape as well as the only one, because it
//! is exactly the seam a person typing `r_ssao_radius 1.5` goes through. A row
//! on the pause panel and a typed line cannot hold two answers that disagree,
//! because there is one cell and both write it.
//!
//! # Ranges are the variable's, not this sample's
//!
//! Every nudge below clamps against [`ConVar::kind`] rather than against a
//! bound written here. `crcbl_render::ssao` is where the radius's and the
//! intensity's limits are argued, and a second copy of them in a sample is a
//! copy that goes stale the day one moves — which is what
//! `every_knob_this_sample_drives_is_declared_by_the_engine` and
//! `a_nudge_stops_at_the_variables_own_bound` are for.
//!
//! # Nothing here is kept
//!
//! There is no state in this module. A [`Knobs`] is a *reading*, taken once a
//! frame for the panel and the summary; the values live in the console's cells
//! and in nothing else.

use crcbl::console::{ConVar, Kind, Value};
use crcbl::render::DebugView;

/// The gather the near side of the seam runs.
pub const TECHNIQUE: &str = "r_ssao_technique";

/// The world-space disc the horizons are swept over.
pub const RADIUS: &str = "r_ssao_radius";

/// The exponent the measured occlusion is raised to.
pub const INTENSITY: &str = "r_ssao_intensity";

/// Where the comparison seam stands, as a fraction of the frame's width.
pub const SPLIT: &str = "r_ssao_split";

/// Whether the gather reports a bent direction beside the scalar.
pub const BENT_NORMALS: &str = "r_ssao_bent_normals";

/// Every variable this sample drives, in the order the panel prints them.
///
/// One list rather than five call sites, because two things walk it: the panel
/// and `reset`, and a variable added to one and forgotten by the other is a knob
/// a run cannot put back.
pub const KNOBS: [&str; 5] = [TECHNIQUE, RADIUS, INTENSITY, SPLIT, BENT_NORMALS];

/// Where the seam stands when it is switched on.
///
/// The middle of the frame, which is what `docs/plan/sample/19-alcove.md`'s
/// milestone 2 asks for: the two techniques over the same geometry, each with
/// half the picture. `nudge_seam` is how it moves off centre.
pub const SEAM_CENTRE: f32 = 0.5;

/// How far one press moves the seam.
const SEAM_STEP: f32 = 0.02;

/// How much one press multiplies or divides the radius by.
///
/// A ratio rather than an addition: the radius spans six doublings between its
/// own bounds, so a fixed step would be a third of the range at one end and
/// imperceptible at the other.
const RADIUS_STEP: f32 = 1.25;

/// How far one press moves the intensity, for [`RADIUS_STEP`]'s reason.
const INTENSITY_STEP: f32 = 1.15;

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
        crcbl::log::error!("alcove: the console refused {name} = {value}: {fault}");
    }
}

/// The names a [`Kind::Enum`] variable accepts, in the order it declares them.
///
/// Empty for any other kind, which no caller here has: [`TECHNIQUE`] is the one
/// enum this sample drives and `the_technique_row_cycles_the_engines_own_set` is
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

/// Multiplies a float variable by `factor` — or divides by it — and clamps the
/// result into the variable's own range.
fn scale(name: &str, factor: f32) {
    let Some((min, max)) = float_range(name) else {
        return;
    };
    let moved = (var(name).get_f32() * factor).clamp(min, max);
    set(name, &Value::Float(moved));
}

/// Widens or narrows the occlusion radius by one press.
pub fn nudge_radius(up: bool) {
    scale(RADIUS, if up { RADIUS_STEP } else { 1.0 / RADIUS_STEP });
}

/// Strengthens or weakens the occlusion by one press.
pub fn nudge_intensity(up: bool) {
    scale(
        INTENSITY,
        if up {
            INTENSITY_STEP
        } else {
            1.0 / INTENSITY_STEP
        },
    );
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

/// Flips the bent-direction switch.
pub fn toggle_bent_normals() {
    set(BENT_NORMALS, &Value::Bool(!var(BENT_NORMALS).get_bool()));
}

/// Where the seam stands, or `None` for a frame comparing nothing.
///
/// **Zero and one are both "off"**, which is `crcbl_render::ssao::split_at`'s
/// own rule: a seam at either edge leaves one side of the frame empty, so the
/// chain records once. Spelled here as well so the panel and the pass agree
/// about what a person is looking at.
#[must_use]
pub fn seam() -> Option<f32> {
    let at = var(SPLIT).get_f32();
    (at > 0.0 && at < 1.0).then_some(at)
}

/// Puts every knob in [`KNOBS`] back to the value the engine declares.
///
/// What `--technique`, `--split` and the keys have moved, undone — which is what
/// a golden run needs between two frames it means to compare, and what the
/// binary does on its way out so a console left mid-experiment does not reach
/// the next process through a settings file.
pub fn reset() {
    for name in KNOBS {
        let var = var(name);
        let default = var.default().clone();
        set(name, &default);
    }
}

/// Whether the frame draws the occlusion channel as grey instead of shading.
///
/// **Read, not kept**, on `apps/lantern`'s terms: the `AO VIEW` row and the
/// console's `debug_view ambient occlusion` are one value —
/// `docs/plan/52-debug-console.md` decision 8 — and [`crcbl::engine::Loop`] is
/// what puts it into force.
#[must_use]
pub fn occlusion_view() -> bool {
    crcbl::debug_view::current() == DebugView::AmbientOcclusion
}

/// Swaps between the shaded picture and the occlusion channel drawn as grey.
pub fn toggle_occlusion_view() {
    crcbl::debug_view::toggle(DebugView::AmbientOcclusion);
}

/// What every knob reads right now.
///
/// A reading rather than a store — see this module's header — taken once per
/// frame by `crate::app` so the panel, the pause menu's labels and the headless
/// summary all report the same instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    /// The gather the near side of the seam runs.
    pub technique: &'static str,
    /// The gather the far side runs: the technique the chain **ships**, read off
    /// the variable's own declared default rather than written down, exactly as
    /// `crcbl_render::ssao`'s own `shipped_technique` does.
    pub shipped: &'static str,
    /// The disc the horizons are swept over, in world units.
    pub radius: f32,
    /// The exponent the measured occlusion is raised to.
    pub intensity: f32,
    /// Where the seam stands, or `None` for a frame comparing nothing.
    pub seam: Option<f32>,
    /// Whether the gather reports a bent direction beside the scalar.
    pub bent_normals: bool,
    /// Whether the frame draws the occlusion channel instead of shading it.
    pub occlusion_view: bool,
}

impl Knobs {
    /// Reads every knob.
    #[must_use]
    pub fn read() -> Self {
        Self {
            technique: var(TECHNIQUE).get_enum(),
            shipped: shipped_technique(),
            radius: var(RADIUS).get_f32(),
            intensity: var(INTENSITY).get_f32(),
            seam: seam(),
            bent_normals: var(BENT_NORMALS).get_bool(),
            occlusion_view: occlusion_view(),
        }
    }

    /// What the panel and the summary call the near side of the seam.
    #[must_use]
    pub fn near_side(&self) -> String {
        format!("{} (console)", self.technique)
    }

    /// The same for the far side, which is only a side at all while the seam is
    /// up.
    ///
    /// **`docs/plan/sample/19-alcove.md`'s milestone 2 is this line.** The engine
    /// half puts a different technique on each side of the seam; the sample's
    /// half is saying which, because two grey pictures side by side name
    /// neither.
    #[must_use]
    pub fn far_side(&self) -> String {
        match self.seam {
            Some(_) => format!("{} (shipped)", self.shipped),
            None => "no seam: one technique".to_string(),
        }
    }

    /// Where the seam is, in words.
    #[must_use]
    pub fn seam_row(&self) -> String {
        match self.seam {
            Some(at) => format!("{at:.2} of the width"),
            None => "OFF".to_string(),
        }
    }
}

/// The technique the chain ships, off [`TECHNIQUE`]'s own declared default.
///
/// `crcbl_render::ssao::shipped_technique`'s rule, applied from outside: a
/// second copy of "what ships" goes stale the day the default moves, and the
/// comparison would then be against a configuration nothing has shipped since.
#[must_use]
pub fn shipped_technique() -> &'static str {
    match var(TECHNIQUE).default() {
        Value::Enum(name) => name,
        // Unreachable by construction: `convar!` builds an enum cell and its
        // default together. This is what a `match` needs.
        _ => "",
    }
}

impl crcbl::ui::DebugModule for Knobs {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("occlusion");
        section.row_str(
            "view",
            if self.occlusion_view {
                "AO ONLY"
            } else {
                "SHADED"
            },
        );
        section.row("radius", format_args!("{:.3} m", self.radius));
        section.row("intensity", format_args!("{:.2}", self.intensity));
        section.row_str("bent normals", if self.bent_normals { "ON" } else { "OFF" });
        section.row_str("seam", &self.seam_row());
        section.row_str("near side", &self.near_side());
        section.row_str("far side", &self.far_side());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises every check that moves a knob, and puts them all back.
    ///
    /// **Every check here takes it, and that is the point.** A
    /// [`ConVar`] is process-global by design and `cargo test` runs a crate's
    /// tests as threads of one process, so two checks that move the radius are
    /// two writers to one cell — which shows up as a flake rather than as a
    /// failure anybody can read. `crcbl::debug_view::for_test` is the same
    /// shape for the same reason.
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
        assert_eq!(KNOBS.len(), 5, "the panel prints one row per knob");
        for name in KNOBS {
            let var = var(name);
            assert_eq!(var.name(), name);
            assert!(!var.help().is_empty(), "{name} has no help line");
        }
        assert!(
            matches!(var(TECHNIQUE).kind(), Kind::Enum(_)),
            "the technique is a name from a set"
        );
        assert!(matches!(var(RADIUS).kind(), Kind::Float { .. }));
        assert!(matches!(var(INTENSITY).kind(), Kind::Float { .. }));
        assert!(matches!(var(SPLIT).kind(), Kind::Float { .. }));
        assert!(matches!(var(BENT_NORMALS).kind(), Kind::Bool));
    }

    /// **Cycling the technique walks the engine's own set**, and comes back
    /// round to where it started.
    ///
    /// Written against [`names`] rather than against two literals, so a third
    /// technique landing in `crcbl-render` is one this sample reaches without a
    /// change here — and a set that shrank to one is caught by the assertion
    /// that it has more than one member, which is what makes the seam a
    /// comparison at all.
    #[test]
    fn the_technique_row_cycles_the_engines_own_set() {
        let _held = held();
        let names = names(TECHNIQUE);
        assert!(
            names.len() > 1,
            "a seam between one technique and itself compares nothing"
        );
        let start = var(TECHNIQUE).get_enum();
        let mut seen = vec![start];
        for _ in 1..names.len() {
            cycle(TECHNIQUE);
            let now = var(TECHNIQUE).get_enum();
            assert!(!seen.contains(&now), "cycling repeated {now} early");
            seen.push(now);
        }
        cycle(TECHNIQUE);
        assert_eq!(var(TECHNIQUE).get_enum(), start, "the cycle must wrap");
        seen.sort_unstable();
        let mut declared = names.to_vec();
        declared.sort_unstable();
        assert_eq!(seen, declared, "cycling must reach every declared name");
    }

    /// **A nudge stops at the variable's own bound**, rather than at one this
    /// sample wrote down.
    ///
    /// Both ends, and for both knobs. The failure this catches is a step that
    /// walks past a limit the console would refuse: the set would fault, the log
    /// would carry a line nobody reads, and the knob would be stuck at whatever
    /// it last held.
    #[test]
    fn a_nudge_stops_at_the_variables_own_bound() {
        let _held = held();
        for (name, nudge) in [
            (RADIUS, nudge_radius as fn(bool)),
            (INTENSITY, nudge_intensity as fn(bool)),
        ] {
            let (min, max) = float_range(name).expect("a float variable");
            for _ in 0..64 {
                nudge(true);
            }
            let top = var(name).get_f32();
            assert!(
                (top - max).abs() < 1e-4,
                "{name} stopped at {top} rather than its own {max}"
            );
            for _ in 0..128 {
                nudge(false);
            }
            let bottom = var(name).get_f32();
            assert!(
                (bottom - min).abs() < 1e-4,
                "{name} stopped at {bottom} rather than its own {min}"
            );
        }
    }

    /// **The seam goes up at the centre, moves, and comes back down**, and a
    /// nudge with the seam down changes nothing.
    ///
    /// The last clause is the one worth guarding: a nudge that wrote a value
    /// while the seam was off would switch the comparison on from a key that
    /// says it moves one.
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

    /// **The panel says which technique each side of the seam is running**, and
    /// says there is no far side when there is no seam.
    ///
    /// `docs/plan/sample/19-alcove.md`'s milestone 2 is this row and this row
    /// alone: the engine puts two techniques either side of a seam, and a
    /// picture of two greys names neither of them.
    #[test]
    fn the_panel_names_the_technique_on_each_side_of_the_seam() {
        let _held = held();
        let shipped = shipped_technique();
        assert!(!shipped.is_empty(), "the technique declares a default");

        let alone = Knobs::read();
        assert!(alone.far_side().contains("no seam"), "{}", alone.far_side());

        // Move the console's technique off the shipped one, which is the state
        // the seam exists to draw.
        cycle(TECHNIQUE);
        toggle_seam();
        let compared = Knobs::read();
        assert_ne!(
            compared.technique, compared.shipped,
            "a seam between one technique and itself is not a comparison"
        );
        assert!(
            compared.near_side().contains(compared.technique),
            "the near row does not name the technique it runs: {}",
            compared.near_side()
        );
        assert!(
            compared.far_side().contains(shipped),
            "the far row does not name the shipped technique: {}",
            compared.far_side()
        );
        assert!(
            compared.seam_row().contains("0.50"),
            "the seam row does not say where it stands: {}",
            compared.seam_row()
        );
    }

    /// **`reset` puts every knob back**, which is what a run that has been
    /// experimented on owes the next frame it means to compare.
    #[test]
    fn reset_puts_every_knob_back_to_the_engines_default() {
        let _held = held();
        let before: Vec<Value> = KNOBS.iter().map(|name| var(name).get()).collect();
        cycle(TECHNIQUE);
        nudge_radius(true);
        nudge_intensity(false);
        toggle_seam();
        toggle_bent_normals();
        let moved: Vec<Value> = KNOBS.iter().map(|name| var(name).get()).collect();
        assert_ne!(before, moved, "nothing moved, so there is nothing to reset");

        reset();
        let after: Vec<Value> = KNOBS.iter().map(|name| var(name).get()).collect();
        assert_eq!(after, before);
        for name in KNOBS {
            assert_eq!(&var(name).get(), var(name).default(), "{name}");
        }
    }
}
