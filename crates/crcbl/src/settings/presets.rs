//! The quality tiers of `docs/plan/39-capabilities.md`'s tier table, as the
//! `[engine.video]` keys they are made of.
//!
//! # A preset is a writer, and it is a command rather than a key
//!
//! The plan's own decision is that "a preset is a writer, not a reader":
//! selecting one writes every individual key it covers, and the effect
//! resolution order never sees the preset at all. That is what keeps it from
//! becoming a fifth layer with its own precedence question.
//!
//! So it is a [`ConCommand`](crcbl_console::ConCommand) and **not** a
//! [`CatalogueKey`](super::CatalogueKey): a key would have to hold a value, and
//! the only value it could hold is a label describing what was written into the
//! other keys. A stored label drifts the moment a player hand-edits one of those
//! keys, and it would have to answer what `quality = custom` writes — which is
//! nothing. A command has no stored value to drift: [`selected`] derives the
//! label from what the readers answer, so the word this prints is true by
//! construction rather than by a comment saying it cannot go stale.
//!
//! **The label is therefore one-shot, not sticky.** Selecting `high` and then
//! moving one covered key reads back as `custom` on the next line, because
//! `custom` here means exactly "the covered keys are not any one tier's set" —
//! which includes a run that has never selected anything.
//!
//! # What a tier covers, and what it is silent about
//!
//! What a tier writes is [`QualityValues`]: the render scale, the antialiasing
//! tier and the volumetric fog switch. Those are the table's rows this tree has
//! an `[engine.video]` key for; every other row names an amount of something
//! with no key and often no renderer half — the shadow atlas's size and light
//! budget, the probe volume's levels, SSR's resolution, contact shadows, ray
//! tracing. A preset writes what it can and is silent about the rest;
//! `docs/backlog.md` carries the list.
//!
//! **A knob that is a console variable and not a settings key is not something
//! a tier can reach**, and that is the shape of the road rather than a gap in
//! this module. `crcbl_render::shadow::cadence`'s `r_shadow_cadence` and
//! `r_shadow_faces`, and `crcbl_render::ssao`'s `r_ssao_slices` and
//! `r_ssao_blur_passes`, are process globals a renderer reads once a frame;
//! the player's file is what a preset writes, and
//! `docs/plan/39-capabilities.md` is explicit that a preset is "a layer of keys
//! and not a second mechanism". So each of those reaches a tier in two steps
//! and in this order: a catalogue key in [`super::catalogue`] with a reader
//! that drives the variable — the road `RENDER_SCALE_KEY` already takes to
//! `ForwardRenderer::set_render_scale` — and then a row in the tier table
//! naming what each column spends. Neither exists, and the second is the one
//! that needs a measurement rather than code.
//!
//! One consequence is worth stating rather than hiding: **`medium` and `high`
//! write the same values**, because every row that separates those two columns
//! is one of the keys this tree does not have.
//! `medium_and_high_hold_the_same_values_until_a_key_separates_them` is the
//! tripwire that goes red on the day one lands.
//!
//! # A device that cannot meet a tier
//!
//! Nothing changes for it, and that is the point of writing keys rather than
//! resolving a preset. A tier is the *player's* ask, written into
//! `[engine.video]`; the device clamp is the last layer of the resolution order
//! and it still runs after this, in
//! [`EffectRequest::resolve`](crcbl_render::EffectRequest::resolve) and in
//! [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy).
//! A preset opens no new path to a frame — it goes through [`super::apply`],
//! key by key, exactly as a settings screen's row does — so there is no tier
//! whose values reach a device the ordinary path would have clamped. None of
//! these keys is gated on a [`Capability`](crcbl_hal::Capability) today
//! either: every backend has a scaled target, a resolve pass and a froxel grid.
//!
//! # Selecting nothing draws what it always drew
//!
//! No tier is a default and nothing selects one at start-up. A run that has not
//! typed `quality` holds none of these keys, so
//! [`video`](super::video) answers
//! [`VideoSettings::unrestricted`](super::VideoSettings::unrestricted) and the
//! frame is the one every golden was blessed at —
//! `a_run_that_selects_nothing_is_on_no_tier_and_asks_for_nothing` is the
//! assertion.

use crcbl_console::{Fault, Value};
use crcbl_render::{Antialiasing, RenderEffects};
use crcbl_store::settings::SettingsStack;

use super::{
    ANTIALIASING_KEY, Applied, ConsoleHost, RENDER_SCALE_KEY, Stage, VIDEO_KEYS, VIDEO_NAMESPACE,
    antialiasing_or_default, apply, render_scale, video_effects,
};

/// The word [`selected`] has no tier for.
///
/// Not a tier and not settable: it is what the label reads when the covered keys
/// are not any one tier's set, which is a hand-edited file, a screen the player
/// has moved one row on, and a fresh run alike.
pub const CUSTOM: &str = "custom";

/// Which [`VIDEO_KEYS`] row carries the fog switch a tier writes.
///
/// The index rather than the string, with the assertion below holding it to the
/// effect it must name — so the table reordered under this file is a build that
/// fails rather than a tier that writes the wrong switch.
const FOG_ROW: usize = 4;

/// The `[engine.video]` key of the fog switch, taken from the one table that
/// spells it.
const FOG_KEY: &str = VIDEO_KEYS[FOG_ROW].0;

const _: () = assert!(
    VIDEO_KEYS[FOG_ROW].1.bits() == RenderEffects::VOLUMETRIC_FOG.bits(),
    "FOG_ROW no longer names the volumetric fog switch: VIDEO_KEYS has been \
     reordered, and a quality tier would write the wrong effect key"
);

/// One column of `docs/plan/39-capabilities.md`'s tier table.
///
/// `ultra` is deliberately absent: the plan's catalogue row names five words,
/// and the table has three columns. A fourth tier with no column would be a
/// name with no values behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityPreset {
    /// The browser, lavapipe and integrated column.
    Low,
    /// The column a device reporting no ray tracing opens on.
    Medium,
    /// The desktop column.
    High,
}

impl QualityPreset {
    /// The tiers, cheapest first — the table's own column order.
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    /// The word a person types and [`selected`] prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// The tier called `name`, or [`None`] — including for [`CUSTOM`], which is
    /// a label rather than something to select.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.name() == name)
    }

    /// What this column of the table says, in the keys this tree can write.
    ///
    /// **Every number is a starting budget to sweep on that tier's hardware**,
    /// which is the tier table's own preamble and not a hedge: these are the
    /// values to measure from, and the sweep moves them here.
    #[must_use]
    pub const fn values(self) -> QualityValues {
        match self {
            // "Render scale 0.75 | Volumetric fog off | Antialiasing FXAA".
            Self::Low => QualityValues {
                render_scale: 0.75,
                antialiasing: Antialiasing::Fxaa,
                volumetric_fog: false,
            },
            // "Render scale 1.0 | Volumetric fog on, half-res froxels" — the
            // half-res froxel grid is its own unbuilt rung, so the switch is all
            // this column can say. The AA cell says CMAA2, which is not built:
            // `docs/plan/49-antialiasing.md`'s eighth decision puts CMAA2 and
            // SMAA 1x in one tier and retires SMAA in the slice that lands
            // CMAA2, so the rung above FXAA is `Smaa` until that day and this
            // constant moves with it.
            Self::Medium | Self::High => QualityValues {
                render_scale: 1.0,
                antialiasing: Antialiasing::Smaa,
                volumetric_fog: true,
            },
        }
    }
}

/// The `[engine.video]` values a tier is made of.
///
/// **`PartialEq` and not `Eq`**, since it holds a float — the same reason
/// [`CatalogueKey`](super::CatalogueKey) gives. The comparison is exact and that
/// is deliberate: both scales in the table are exact binary fractions, and the
/// reader clamps to the same range the writer wrote through, so a value that
/// came out of a tier and back through the file is the value that went in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QualityValues {
    /// [`super::RENDER_SCALE_KEY`].
    pub render_scale: f32,
    /// [`super::ANTIALIASING_KEY`], as the rung rather than the word.
    pub antialiasing: Antialiasing,
    /// The [`VIDEO_KEYS`] fog switch: whether the player allows the froxel pass.
    pub volumetric_fog: bool,
}

/// What the engine's own readers answer for the keys a tier covers.
///
/// Through the readers rather than off the stack, so this is what a frame would
/// be built from — including the clamp and the default an absent key leaves. It
/// is what [`selected`] compares, which is why the label cannot claim a tier the
/// frame is not on.
#[must_use]
pub fn current_values(stack: &SettingsStack) -> QualityValues {
    QualityValues {
        render_scale: render_scale(stack),
        antialiasing: antialiasing_or_default(stack),
        volumetric_fog: video_effects(stack).contains(RenderEffects::VOLUMETRIC_FOG),
    }
}

/// The tier `stack`'s covered keys hold, or [`None`] for [`CUSTOM`].
///
/// Derived rather than stored — see the [module docs](self). [`QualityPreset::ALL`]
/// is searched cheapest first, so the answer for two tiers holding one set of
/// values is the cheaper of them; today that is `medium` for a stack `high`
/// wrote, and the two mean the same values.
#[must_use]
pub fn selected(stack: &SettingsStack) -> Option<QualityPreset> {
    let held = current_values(stack);
    QualityPreset::ALL
        .into_iter()
        .find(|tier| tier.values() == held)
}

/// What [`quality`] prints for `stack`: every tier whose values it holds, or
/// [`CUSTOM`].
///
/// **Every tier and not just [`selected`]'s**, because two columns of the table
/// can say the same thing — `medium` and `high` do today — and a run that
/// selected `high` would otherwise be told it is on `medium`, which reads as a
/// write that went somewhere else. Naming both says what actually happened, and
/// the second name disappears on its own the day a key separates the columns.
#[must_use]
pub fn label(stack: &SettingsStack) -> String {
    let held = current_values(stack);
    let names: Vec<&str> = QualityPreset::ALL
        .into_iter()
        .filter(|tier| tier.values() == held)
        .map(QualityPreset::name)
        .collect();
    if names.is_empty() {
        return CUSTOM.to_owned();
    }
    names.join(", ")
}

/// Write `preset`'s keys into `stack` and apply them through `stage`.
///
/// **Key by key through [`super::apply`]**, which is the one place a settings
/// key is written and applied together — so a tier reaches a frame by exactly
/// the path a settings screen's row does, and cannot grow a second one that
/// forgets the stage or the clamp.
///
/// [`Applied::NextStart`] if any key did not reach this process, which on one
/// [`Stage`] is all of them or none: every one of them lands through
/// [`Stage::apply_video`].
///
/// # Errors
///
/// [`super::apply`]'s. The values are the table's own and pass every
/// [`Kind`](crcbl_console::Kind) check by construction, so what is left is a
/// storage error — and that cannot leave a tier half written, because every key
/// here shares the `engine.video` prefix a failing write would be refused on, so
/// the first one would already have failed.
pub fn select(
    stack: &mut SettingsStack,
    preset: QualityPreset,
    stage: &mut dyn Stage,
) -> Result<Applied, Fault> {
    let values = preset.values();
    let mut reached = Applied::Live;
    for (name, value) in [
        (RENDER_SCALE_KEY, Value::Float(values.render_scale)),
        (ANTIALIASING_KEY, Value::Enum(values.antialiasing.name())),
        (FOG_KEY, Value::Bool(values.volumetric_fog)),
    ] {
        let key = format!("{VIDEO_NAMESPACE}.{name}");
        if apply(stack, &key, &value, stage)? == Applied::NextStart {
            reached = Applied::NextStart;
        }
    }
    Ok(reached)
}

/// One line describing what the covered keys hold.
fn values_line(values: QualityValues) -> String {
    format!(
        "{RENDER_SCALE_KEY} = {}, {ANTIALIASING_KEY} = {}, {FOG_KEY} = {}",
        values.render_scale,
        values.antialiasing.name(),
        values.volumetric_fog,
    )
}

crcbl_console::concommand! {
    /// The quality tier the video keys hold — `quality medium` writes one, bare prints it.
    pub fn quality(cx, args) {
        if args.is_empty() {
            let (name, values) = {
                let host = cx
                    .host()
                    .downcast_ref::<ConsoleHost>()
                    .expect("the engine's console is only ever run over a `ConsoleHost`");
                let stack = host.stack();
                (label(&stack), current_values(&stack))
            };
            cx.print(format!("quality = {name} — {}", values_line(values)));
            if name == CUSTOM {
                cx.print(format!(
                    "`{CUSTOM}` is no tier: try {}, which write {}",
                    QualityPreset::ALL.map(QualityPreset::name).join(", "),
                    [RENDER_SCALE_KEY, ANTIALIASING_KEY, FOG_KEY].join(", "),
                ));
            }
            return Ok(());
        }

        let typed = args.join(" ");
        let preset = QualityPreset::from_name(&typed).ok_or_else(|| {
            Fault::new(format!(
                "`{typed}` is not a quality tier — try {}",
                QualityPreset::ALL.map(QualityPreset::name).join(", "),
            ))
        })?;
        {
            let host = cx
                .host_mut()
                .downcast_mut::<ConsoleHost>()
                .expect("the engine's console is only ever run over a `ConsoleHost`");
            let mut stack = host.stack.stack_mut();
            // The answer is dropped rather than reported, because from here it
            // is always `Applied::Live`: a `ConsoleHost`'s stage is a
            // `Deferred`, which records every seam and refuses none. A caller
            // with a stage that can answer `Unsupported` calls `select`
            // directly and reads it.
            select(&mut stack, preset, &mut host.pending)?;
        }
        cx.print(format!(
            "quality = {} — {}",
            preset.name(),
            values_line(preset.values()),
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::settings::tests::stack_from;
    use crate::settings::{Deferred, VideoSettings, video};
    use crcbl_console::{Context, Registry};

    /// A stack nothing has written, and a stage that records what a write
    /// reached.
    fn empty() -> (SettingsStack, Deferred) {
        (stack_from(""), Deferred::new())
    }

    /// **The tiers a person can type are the tiers that have values**, both
    /// ways, so a name added to one half and not the other is a red test rather
    /// than a word the console accepts and does nothing for.
    #[test]
    fn every_tier_answers_to_its_own_name() {
        assert!(
            !QualityPreset::ALL.is_empty(),
            "an empty ladder proves nothing"
        );
        for tier in QualityPreset::ALL {
            assert_eq!(QualityPreset::from_name(tier.name()), Some(tier));
        }
        assert_eq!(
            QualityPreset::from_name(CUSTOM),
            None,
            "`custom` is not a tier"
        );
        assert_eq!(QualityPreset::from_name("ultra"), None);
    }

    /// **`low` and `medium` differ in every key a tier covers.**
    ///
    /// The check this file would otherwise not have: a preset whose columns
    /// happened to agree on a key would pass every test below while proving
    /// nothing about that key, which is why this asserts each disagreement
    /// one at a time rather than comparing the structs.
    #[test]
    fn low_and_medium_differ_in_every_key_a_tier_covers() {
        let low = QualityPreset::Low.values();
        let medium = QualityPreset::Medium.values();
        assert!(
            (low.render_scale - medium.render_scale).abs() > f32::EPSILON,
            "the two tiers draw at the same scale, so the scale proves nothing",
        );
        assert_ne!(
            low.antialiasing, medium.antialiasing,
            "the two tiers resolve with the same filter",
        );
        assert_ne!(
            low.volumetric_fog, medium.volumetric_fog,
            "the two tiers allow the same fog",
        );
    }

    /// **`medium` and `high` hold the same values**, and this is the tripwire
    /// for the day they stop.
    ///
    /// Every tier-table row that separates those two columns — the shadow
    /// atlas's size and light budget, the probe volume's levels, SSR's
    /// resolution, ray tracing — is a knob with no `[engine.video]` key, so
    /// there is nothing for the two columns to disagree about yet. A key that
    /// lands and separates them reddens this, which is when
    /// `docs/plan/43-render-standards.md`'s row and this module's header both
    /// want editing.
    #[test]
    fn medium_and_high_hold_the_same_values_until_a_key_separates_them() {
        assert_eq!(
            QualityPreset::Medium.values(),
            QualityPreset::High.values(),
            "the two columns now differ in a key a tier writes: give `high` its \
             own arm and delete this test",
        );
    }

    /// **Selecting a tier moves what the engine's own readers answer**, for
    /// every key it covers and for each tier.
    ///
    /// Asserted through [`render_scale`], [`antialiasing_or_default`] and
    /// [`video_effects`] — the functions a start-up builds a frame from — rather
    /// than through the struct [`select`] was handed, which would pass whether
    /// or not a key was written.
    #[test]
    fn selecting_a_tier_moves_what_the_engine_reads() {
        for tier in QualityPreset::ALL {
            let (mut stack, mut stage) = empty();
            select(&mut stack, tier, &mut stage).expect("a memory stack accepts a write");
            let wanted = tier.values();
            assert!(
                (render_scale(&stack) - wanted.render_scale).abs() < f32::EPSILON,
                "{tier:?} left the scale at {}",
                render_scale(&stack),
            );
            assert_eq!(
                antialiasing_or_default(&stack),
                wanted.antialiasing,
                "{tier:?}"
            );
            assert_eq!(
                video_effects(&stack).contains(RenderEffects::VOLUMETRIC_FOG),
                wanted.volumetric_fog,
                "{tier:?}",
            );
        }
    }

    /// **A run that selects nothing asks for nothing**, which is what makes a
    /// preset opt-in rather than a change to every frame in the tree.
    ///
    /// Both halves: the stack holds none of the covered keys, and the whole
    /// section reads back as the unrestricted one every golden was blessed at.
    #[test]
    fn a_run_that_selects_nothing_is_on_no_tier_and_asks_for_nothing() {
        let stack = stack_from("");
        assert_eq!(selected(&stack), None);
        assert_eq!(label(&stack), CUSTOM);
        for name in [RENDER_SCALE_KEY, ANTIALIASING_KEY, FOG_KEY] {
            assert!(
                !stack.contains(&format!("{VIDEO_NAMESPACE}.{name}")),
                "`{name}` was written by nobody selecting anything",
            );
        }
        assert_eq!(video(&stack), VideoSettings::unrestricted());
    }

    /// **The label is derived, so it is a tier only while every covered key
    /// still holds that tier's value.**
    ///
    /// One key at a time, because a label that compared only the scale would
    /// pass a check that moved the filter — which is exactly the drift a stored
    /// label has and this one does not.
    #[test]
    fn moving_any_covered_key_takes_the_label_off_its_tier() {
        let moves: [(&str, Value); 3] = [
            (RENDER_SCALE_KEY, Value::Float(0.5)),
            (ANTIALIASING_KEY, Value::Enum(Antialiasing::None.name())),
            (FOG_KEY, Value::Bool(false)),
        ];
        for (name, value) in moves {
            let (mut stack, mut stage) = empty();
            select(&mut stack, QualityPreset::Medium, &mut stage)
                .expect("a memory stack accepts a write");
            assert_eq!(
                selected(&stack),
                Some(QualityPreset::Medium),
                "the tier did not take",
            );
            apply(
                &mut stack,
                &format!("{VIDEO_NAMESPACE}.{name}"),
                &value,
                &mut stage,
            )
            .unwrap_or_else(|fault| panic!("`{name}` refused {value}: {fault}"));
            assert_eq!(
                label(&stack),
                CUSTOM,
                "moving `{name}` left the label claiming a tier the keys are not on",
            );
        }
    }

    /// **A set of values two columns share is printed as both of them.**
    ///
    /// `medium` and `high` are one set today, so a run that selected `high`
    /// must not be told it is on `medium` — which is what a single-answer label
    /// would say, since `selected` takes the cheapest match. The day a key
    /// separates the columns this reads as one name again, with nothing to
    /// change here.
    #[test]
    fn a_set_of_values_two_columns_share_is_printed_as_both() {
        let (mut stack, mut stage) = empty();
        select(&mut stack, QualityPreset::High, &mut stage)
            .expect("a memory stack accepts a write");
        let printed = label(&stack);
        for tier in QualityPreset::ALL {
            assert_eq!(
                printed.contains(tier.name()),
                tier.values() == QualityPreset::High.values(),
                "`{printed}` disagrees with which columns hold these values about {tier:?}",
            );
        }
    }

    /// **A tier written and read back through the file is the same tier**,
    /// which is what makes the label survive a restart with no stored word.
    #[test]
    fn a_tier_survives_the_round_trip_through_the_stack() {
        for tier in QualityPreset::ALL {
            let (mut stack, mut stage) = empty();
            select(&mut stack, tier, &mut stage).expect("a memory stack accepts a write");
            assert_eq!(current_values(&stack), tier.values(), "{tier:?}");
            // `medium` and `high` are one set of values today, so the label is
            // the cheaper of the two rather than the one that was selected —
            // `selected`'s own documented order.
            assert_eq!(
                selected(&stack).map(QualityPreset::values),
                Some(tier.values()),
                "{tier:?}",
            );
        }
    }

    /// **A tier written through a host with no seam is still written**, and
    /// says so.
    ///
    /// [`Applied::NextStart`] is the answer `apps/options` draws its "next
    /// start" mark from, and it is the whole reason [`select`] returns
    /// something: a stage that reaches nothing must not look like a refusal,
    /// which would leave the player's file unwritten.
    #[test]
    fn a_stage_with_no_seam_writes_the_tier_and_reports_the_next_start_up() {
        /// Every [`Stage`] method left at its default, which is the bundle with
        /// no renderer.
        #[derive(Debug)]
        struct NoSeam;
        impl Stage for NoSeam {}

        let mut stack = stack_from("");
        assert_eq!(
            select(&mut stack, QualityPreset::Low, &mut NoSeam)
                .expect("a memory stack accepts a write"),
            Applied::NextStart,
        );
        assert_eq!(
            selected(&stack),
            Some(QualityPreset::Low),
            "the tier did not reach the file it was written to",
        );
    }

    /// The registry a command test runs in: the built-ins, and nothing else the
    /// command reads.
    fn registry() -> Registry {
        Registry::gather(&[]).expect("the built-in table alone has no duplicate")
    }

    /// **`quality` with no argument prints the label and the values behind it**,
    /// and says what to type when there is no tier.
    #[test]
    fn the_bare_command_prints_the_label_and_what_it_is_made_of() {
        let registry = registry();
        let mut host = ConsoleHost::new(stack_from(""));
        let mut cx = Context::new(&registry, &mut host);
        quality.run(&mut cx, &[]).expect("printing cannot fault");
        let printed = cx.into_lines();
        assert!(
            printed[0].contains(&format!("quality = {CUSTOM}")),
            "the label is missing: {printed:?}",
        );
        assert!(
            printed[0].contains(RENDER_SCALE_KEY) && printed[0].contains(FOG_KEY),
            "the values behind the label are missing: {printed:?}",
        );
        assert!(
            printed.iter().any(|line| line.contains("medium")),
            "nothing said what to type instead: {printed:?}",
        );
    }

    /// **`quality medium` writes the tier, and the console then reads it back.**
    #[test]
    fn the_command_writes_a_tier_and_reads_it_back() {
        let registry = registry();
        let mut host = ConsoleHost::new(stack_from(""));
        {
            let mut cx = Context::new(&registry, &mut host);
            quality
                .run(&mut cx, &["medium"])
                .expect("`medium` is a tier");
            let printed = cx.into_lines();
            assert!(
                printed[0].starts_with("quality = medium"),
                "the write did not report the tier: {printed:?}",
            );
        }
        assert_eq!(
            selected(&host.stack()),
            Some(QualityPreset::Medium),
            "the write did not reach the stack",
        );
        let mut cx = Context::new(&registry, &mut host);
        quality.run(&mut cx, &[]).expect("printing cannot fault");
        assert!(
            cx.lines()[0].starts_with("quality = medium"),
            "the label did not read the tier back: {:?}",
            cx.lines(),
        );
    }

    /// **A word that is not a tier is refused and writes nothing.**
    ///
    /// `ultra` by name: the plan's catalogue row names it and the tier table has
    /// no column for it, so it is the word a person is most likely to try.
    #[test]
    fn a_word_that_is_not_a_tier_is_refused_and_the_stack_is_untouched() {
        let registry = registry();
        let mut host = ConsoleHost::new(stack_from(""));
        let mut cx = Context::new(&registry, &mut host);
        let fault = quality
            .run(&mut cx, &["ultra"])
            .expect_err("`ultra` has no column in the tier table");
        assert!(
            fault.to_string().contains("ultra") && fault.to_string().contains("low"),
            "the refusal named neither the word nor the tiers: {fault}",
        );
        drop(cx);
        assert!(
            !host
                .stack()
                .contains(&format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}")),
            "a refused tier still wrote a key",
        );
    }
}
