//! Applying a key: [`apply`] writes one catalogue key and hands it to a
//! [`Stage`], the seams a host has for showing it.

use crcbl_audio::mixer::Bus;
use crcbl_console::{Fault, Value};
use crcbl_render::{Antialiasing, shadow::Filter};
use crcbl_store::StorageError;
use crcbl_store::settings::SettingsStack;

use super::engine_audio::{AUDIO_NAMESPACE, set_audio_gain};
use super::engine_video::{
    ANISOTROPIC_FILTERING_KEY, ANTIALIASING_KEY, FRAME_LIMIT_KEY, RENDER_SCALE_KEY,
    SHADOW_FILTER_KEY, SSAO_BENT_NORMALS_KEY, SSAO_BLUR_PASSES_KEY, SSAO_SLICES_KEY, UI_SCALE_KEY,
    VideoSettings, effect_keys, set_anisotropic_filtering, set_antialiasing, set_frame_limit,
    set_render_scale, set_shadow_filter, set_ssao_bent_normals, set_ssao_blur_passes,
    set_ssao_slices, set_ui_scale, set_video_effects, ui_scale, video, video_effects,
};
use super::key_catalogue::{KeyStatus, catalogued};
use crate::engine::FrameLimit;

#[cfg(doc)]
use crcbl_console::{Binding, Kind};
#[cfg(doc)]
use std::any::Any;

/// A seam the host does not have.
///
/// Not an error: a settings screen with no renderer, a headless run with no
/// mixer and the engine's own loop fixture are all hosts that legitimately
/// cannot show a key, and every one of them still wants the key written. It is
/// reported rather than swallowed because the alternative is
/// "not implemented" arriving as "applied" — the failure
/// topic 40 names for counters and this file's [`KeyStatus`]
/// names for keys.
///
/// **A `Result<(), Unsupported>` rather than a three-armed enum**, because the
/// two outcomes are exactly "it happened" and "there is nothing here to happen
/// on": `?` composes them at a call site that has several seams to reach, and
/// `#[must_use]` on `Result` is what stops a bundle's forward silently dropping
/// one. [`Applied`] is the answer [`apply`] gives, and it is where the shades in
/// between belong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unsupported;

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("this host has no seam to apply that through")
    }
}

impl std::error::Error for Unsupported {}

/// What a settings write reaches once the stack holds it.
///
/// # Why this is not [`GameGpu`](crate::engine::GameGpu)
///
/// Two reasons, and either alone would decide it. `GameGpu` is `Sized` and
/// takes `self` by value in `destroy`, so it is not object-safe and there is no
/// `&mut dyn GameGpu` for [`apply`] to take; and a settings key reaches more
/// than a renderer — the mixer and the loop's clock are seams no GPU bundle
/// owns. So the bundle keeps the pair debug-console decision 3
/// (`docs/notes/tooling.md`) puts on it, [`GpuStage`] is the one line that
/// forwards to it, and this is the vocabulary [`apply`] speaks.
///
/// **Every method defaults to [`Unsupported`]**, so an implementor writes only
/// the seams it actually has and a caller is told which of them did nothing
/// rather than left to assume.
pub trait Stage {
    /// Hand the whole `[engine.video]` section to the renderer.
    ///
    /// The whole section rather than the key that moved: the renderer's own
    /// state is the resolved set, the scale and the sampler together, and a
    /// caller that applied one key would have to know which of the three it
    /// touched.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] where this host has no renderer.
    fn apply_video(&mut self, video: &VideoSettings) -> Result<(), Unsupported> {
        let _ = video;
        Err(Unsupported)
    }

    /// Move one bus's gain on the running mixer.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] where this host has no mixer.
    fn set_bus_gain(&mut self, bus: Bus, gain: f32) -> Result<(), Unsupported> {
        let _ = (bus, gain);
        Err(Unsupported)
    }

    /// Put the loop under a new frame-rate ceiling.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] where this host has no clock to re-pace — which is every
    /// host in this workspace today, since
    /// [`Loop`](crate::engine::Loop) takes its limit when it is built. See
    /// `docs/backlog.md`.
    fn set_frame_limit(&mut self, limit: FrameLimit) -> Result<(), Unsupported> {
        let _ = limit;
        Err(Unsupported)
    }

    /// Draw the host's own UI at a new `[engine.video] ui_scale` multiplier,
    /// already clamped as [`ui_scale`] reads it.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] where this host draws no UI at a scale — every host but
    /// the [`Loop`](crate::engine::Loop)'s console, whose [`Deferred`] carries
    /// it to the loop's own menus, console and overlay.
    fn set_ui_scale(&mut self, scale: f32) -> Result<(), Unsupported> {
        let _ = scale;
        Err(Unsupported)
    }
}

/// The [`Stage`] a GPU bundle is: `[engine.video]` reaches the renderer through
/// [`GameGpu::apply_video`](crate::engine::GameGpu::apply_video).
///
/// One line of forwarding, and the whole of the path from a typed settings write
/// to a live frame. A bundle with no renderer inherits that method's default and
/// this reports [`Unsupported`] without the caller having to ask which kind of
/// bundle it holds.
#[derive(Debug)]
pub struct GpuStage<'a, G: crate::engine::GameGpu>(pub &'a mut G);

impl<G: crate::engine::GameGpu> Stage for GpuStage<'_, G> {
    fn apply_video(&mut self, video: &VideoSettings) -> Result<(), Unsupported> {
        self.0.apply_video(video)
    }
}

/// A [`Stage`] that records what it was asked to do, for a caller that cannot
/// hold the thing it would apply through.
///
/// **The console's host is the caller.** A [`Binding`] reaches its host as
/// `&mut dyn Any`, and [`Any`] is implemented only for `'static` types — so the
/// state a binding writes cannot hold a borrow of the renderer or the mixer,
/// both of which live for a frame. This records the write instead and the loop
/// drains it where the bundle is in hand, which is
/// [`HostedGame::take_pending_frame_limit`](crate::engine::HostedGame::take_pending_frame_limit)'s
/// arrangement already.
///
/// It keeps the **latest** ask per seam rather than a queue: two writes to one
/// key in a frame are one thing to apply, and applying the first would be
/// drawing a value the player has already moved off.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Deferred {
    video: Option<VideoSettings>,
    gains: [Option<f32>; Bus::ALL.len()],
    frame_limit: Option<FrameLimit>,
    ui_scale: Option<f32>,
}

impl Deferred {
    /// Nothing recorded.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            video: None,
            gains: [None; Bus::ALL.len()],
            frame_limit: None,
            ui_scale: None,
        }
    }

    /// The `[engine.video]` section a write asked for, taken.
    pub const fn take_video(&mut self) -> Option<VideoSettings> {
        self.video.take()
    }

    /// The bus gains a write asked for, taken, in [`Bus::ALL`]'s order.
    pub const fn take_gains(&mut self) -> [Option<f32>; Bus::ALL.len()] {
        std::mem::replace(&mut self.gains, [None; Bus::ALL.len()])
    }

    /// The frame ceiling a write asked for, taken.
    pub const fn take_frame_limit(&mut self) -> Option<FrameLimit> {
        self.frame_limit.take()
    }

    /// The UI multiplier a write asked for, taken.
    pub const fn take_ui_scale(&mut self) -> Option<f32> {
        self.ui_scale.take()
    }

    /// Whether anything is waiting to be applied.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.video.is_none()
            && self.frame_limit.is_none()
            && self.ui_scale.is_none()
            && self.gains.iter().all(Option::is_none)
    }
}

impl Stage for Deferred {
    fn apply_video(&mut self, video: &VideoSettings) -> Result<(), Unsupported> {
        self.video = Some(*video);
        Ok(())
    }

    fn set_bus_gain(&mut self, bus: Bus, gain: f32) -> Result<(), Unsupported> {
        self.gains[bus.index()] = Some(gain);
        Ok(())
    }

    fn set_frame_limit(&mut self, limit: FrameLimit) -> Result<(), Unsupported> {
        self.frame_limit = Some(limit);
        Ok(())
    }

    fn set_ui_scale(&mut self, scale: f32) -> Result<(), Unsupported> {
        self.ui_scale = Some(scale);
        Ok(())
    }
}

/// How far a write got.
///
/// The distinction `apps/options`' rows already draw with their "next start"
/// mark, made into a value so every caller draws it the same way: the stack
/// holds the key either way, and the question is whether anything in *this*
/// process shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// Written, and the thing that shows it has been told.
    Live,
    /// Written, and this host has no seam for it — the next start-up reads it.
    NextStart,
}

/// Write one catalogue key and apply it through `stage`.
///
/// **The one place a settings key is written and applied together**, and the
/// reason debug-console decision 3 (`docs/notes/tooling.md`) asked for it:
/// until this existed the fan-out was `apps/options`', per key, so a console —
/// or a second screen — would have had to copy it, and a copy is where the two
/// drift.
///
/// One function with a match rather than a function per key, because every arm
/// is the same three steps in the same order (coerce, write, apply) and the
/// spelling of each key's writer is the only thing that differs; a function per
/// key would be sixteen bodies that must not disagree about the order.
///
/// # What it refuses
///
/// - A key the catalogue does not name.
/// - A [`KeyStatus::Named`] key — nothing reads it, so writing it would be a
///   value the player set and no frame ever shows.
/// - A value the key's [`Kind`] refuses: the wrong shape, or outside the range
///   the setter clamps to.
/// - A storage error from the write, which leaves the stack as it was.
///
/// A [`Stage`] that answers [`Unsupported`] is **not** a refusal: the key is
/// written and the answer is [`Applied::NextStart`].
///
/// # Errors
///
/// A [`Fault`] naming the key, on each of the terms above.
pub fn apply(
    stack: &mut SettingsStack,
    key: &str,
    value: &Value,
    stage: &mut dyn Stage,
) -> Result<Applied, Fault> {
    let entry = catalogued(key)
        .ok_or_else(|| Fault::new(format!("`{key}` is not a key the engine defines")))?;
    if entry.status == KeyStatus::Named {
        return Err(Fault::new(format!(
            "`{key}`: nothing reads this yet, so setting it would change no frame"
        )));
    }
    entry.kind.check(key, value)?;
    let storage = |error: StorageError| Fault::new(error.to_string());

    if let Some(bus) = Bus::ALL
        .into_iter()
        .find(|bus| entry.key == format!("{AUDIO_NAMESPACE}.{}", bus.settings_key()))
    {
        let Value::Float(gain) = *value else {
            unreachable!("an audio key is a float kind, which `check` has already held it to")
        };
        set_audio_gain(stack, bus, gain).map_err(storage)?;
        return Ok(reached(stage.set_bus_gain(bus, gain)));
    }

    match entry.name {
        FRAME_LIMIT_KEY => {
            let Value::Int(rate) = *value else {
                unreachable!("the frame limit is an int kind, which `check` has held it to")
            };
            let rate = u32::try_from(rate)
                .map_err(|_| Fault::new(format!("`{key}`: {rate} is not a frame rate")))?;
            let limit = FrameLimit::fps(rate);
            set_frame_limit(stack, limit).map_err(storage)?;
            Ok(reached(stage.set_frame_limit(limit)))
        }
        ANTIALIASING_KEY => {
            let Value::Enum(name) = *value else {
                unreachable!("the tier is an enum kind, which `check` has held it to")
            };
            let tier = Antialiasing::from_name(name)
                .expect("`check` has already held the value to `ANTIALIASING_NAMES`");
            set_antialiasing(stack, tier).map_err(storage)?;
            Ok(reached(stage.apply_video(&video(stack))))
        }
        SHADOW_FILTER_KEY => {
            let Value::Enum(name) = *value else {
                unreachable!("the filter is an enum kind, which `check` has held it to")
            };
            let filter = Filter::from_name(name)
                .expect("`check` has already held the value to `SHADOW_FILTER_NAMES`");
            set_shadow_filter(stack, filter).map_err(storage)?;
            Ok(reached(stage.apply_video(&video(stack))))
        }
        SSAO_SLICES_KEY => {
            let Value::Int(slices) = *value else {
                unreachable!()
            };
            set_ssao_slices(stack, slices).map_err(storage)?;
            Ok(reached(stage.apply_video(&video(stack))))
        }
        SSAO_BLUR_PASSES_KEY => {
            let Value::Int(passes) = *value else {
                unreachable!()
            };
            set_ssao_blur_passes(stack, passes).map_err(storage)?;
            Ok(reached(stage.apply_video(&video(stack))))
        }
        SSAO_BENT_NORMALS_KEY => {
            let Value::Bool(bent_normals) = *value else {
                unreachable!()
            };
            set_ssao_bent_normals(stack, bent_normals).map_err(storage)?;
            Ok(reached(stage.apply_video(&video(stack))))
        }
        RENDER_SCALE_KEY => {
            let Value::Float(scale) = *value else {
                unreachable!("the scale is a float kind, which `check` has held it to")
            };
            set_render_scale(stack, scale).map_err(storage)?;
            Ok(reached(stage.apply_video(&video(stack))))
        }
        ANISOTROPIC_FILTERING_KEY => {
            let Value::Float(anisotropy) = *value else {
                unreachable!("the anisotropy is a float kind, which `check` has held it to")
            };
            set_anisotropic_filtering(stack, anisotropy).map_err(storage)?;
            Ok(reached(stage.apply_video(&video(stack))))
        }
        // Not the renderer's: the host that draws a UI at a scale is told,
        // with the multiplier the file now reads back.
        UI_SCALE_KEY => {
            let Value::Float(scale) = *value else {
                unreachable!("the UI scale is a float kind, which `check` has held it to")
            };
            set_ui_scale(stack, scale).map_err(storage)?;
            Ok(reached(stage.set_ui_scale(ui_scale(stack))))
        }
        // Every remaining `Read` key is an effect switch, whose entry in the
        // catalogue is derived from that table — so a name that reaches here and
        // matches nothing is a key catalogued as read with no writer, which the
        // catalogue tests already refuse.
        name => {
            let Value::Bool(on) = *value else {
                unreachable!("an effect key is a bool kind, which `check` has held it to")
            };
            let (_, effect) = effect_keys()
                .find(|(candidate, _)| *candidate == name)
                .expect("every `Read` video key not matched above is an effect switch");
            let mut effects = video_effects(stack);
            effects.set(effect, on);
            set_video_effects(stack, effects).map_err(storage)?;
            Ok(reached(stage.apply_video(&video(stack))))
        }
    }
}

/// [`Applied`] from what a [`Stage`] answered.
const fn reached(outcome: Result<(), Unsupported>) -> Applied {
    match outcome {
        Ok(()) => Applied::Live,
        Err(Unsupported) => Applied::NextStart,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_render::RenderEffects;

    use crate::settings::tests::{Recorder, stack_from};
    use crate::settings::{VIDEO_KEYS, VIDEO_NAMESPACE, render_scale};

    /// **A value outside a key's kind is refused before it reaches the file.**
    #[test]
    fn a_value_outside_its_kind_is_refused_and_the_stack_is_untouched() {
        let key = format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}");
        let mut stack = stack_from("");
        let mut stage = Recorder::default();
        let fault = apply(&mut stack, &key, &Value::Float(0.05), &mut stage)
            .expect_err("below the renderer's floor");
        assert!(fault.message().contains("outside"), "{}", fault.message());
        assert!(!stack.contains(&key), "a refused write reached the file");
        assert!(
            stage.video.is_empty(),
            "a refused write reached the renderer"
        );

        let fault = apply(&mut stack, &key, &Value::Bool(true), &mut stage)
            .expect_err("a scale is not a bool");
        assert_eq!(fault.message(), format!("`{key}` is a float, not a bool"),);
    }

    /// **A key the engine does not define is refused by name.**
    #[test]
    fn a_key_the_engine_does_not_define_cannot_be_applied() {
        let mut stack = stack_from("");
        let mut stage = Recorder::default();
        let fault = apply(
            &mut stack,
            "engine.video.shadow",
            &Value::Bool(true),
            &mut stage,
        )
        .expect_err("a typo is not a key");
        assert!(
            fault.message().contains("is not a key the engine defines"),
            "{}",
            fault.message()
        );
    }

    /// **Each half of the catalogue reaches the seam that shows it**, and the
    /// value the seam is handed is the one the file now holds.
    ///
    /// Reading the stage's record rather than the stack is the point: the
    /// stack half is `a_saved_video_section_reads_back_unchanged`'s, and this
    /// is the half that would silently be a no-op if `apply` wrote the key and
    /// told nobody.
    #[test]
    fn a_write_reaches_the_seam_that_shows_it() {
        let mut stack = stack_from("");
        let mut stage = Recorder {
            has_clock: true,
            ..Recorder::default()
        };

        let key = format!("{VIDEO_NAMESPACE}.{ANTIALIASING_KEY}");
        assert_eq!(
            apply(&mut stack, &key, &Value::Enum("cmaa2"), &mut stage),
            Ok(Applied::Live)
        );
        assert_eq!(
            stage
                .video
                .last()
                .expect("the renderer was told")
                .antialiasing,
            Some(Antialiasing::Cmaa2)
        );

        let key = format!("{VIDEO_NAMESPACE}.{SHADOW_FILTER_KEY}");
        assert_eq!(
            apply(&mut stack, &key, &Value::Enum("disc"), &mut stage),
            Ok(Applied::Live)
        );
        assert_eq!(
            stage
                .video
                .last()
                .expect("the renderer was told")
                .shadow_filter,
            Filter::Disc
        );

        let key = format!("{VIDEO_NAMESPACE}.{}", VIDEO_KEYS[3].0);
        assert_eq!(
            apply(&mut stack, &key, &Value::Bool(false), &mut stage),
            Ok(Applied::Live)
        );
        let video = *stage.video.last().expect("the renderer was told");
        assert!(!video.effects.contains(RenderEffects::BLOOM));
        assert!(
            video.effects.contains(RenderEffects::SHADOWS),
            "one switch took the others with it"
        );
        // The tier survived the second write, which is what proves the stage is
        // handed the section rather than the key.
        assert_eq!(video.antialiasing, Some(Antialiasing::Cmaa2));
        assert_eq!(video.shadow_filter, Filter::Disc);

        let key = format!("{VIDEO_NAMESPACE}.{FRAME_LIMIT_KEY}");
        assert_eq!(
            apply(&mut stack, &key, &Value::Int(60), &mut stage),
            Ok(Applied::Live)
        );
        assert_eq!(stage.limits, [FrameLimit::fps(60)]);

        // The two float rows, whose arms are their own: each hands the
        // renderer the section, and the section carries the other's value.
        let key = format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}");
        assert_eq!(
            apply(&mut stack, &key, &Value::Float(0.5), &mut stage),
            Ok(Applied::Live)
        );
        let video = *stage.video.last().expect("the renderer was told");
        assert!((video.render_scale - 0.5).abs() < f32::EPSILON);
        let key = format!("{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}");
        assert_eq!(
            apply(&mut stack, &key, &Value::Float(4.0), &mut stage),
            Ok(Applied::Live)
        );
        let video = *stage.video.last().expect("the renderer was told");
        assert!((video.anisotropic_filtering - 4.0).abs() < f32::EPSILON);
        assert!(
            (video.render_scale - 0.5).abs() < f32::EPSILON,
            "the scale did not survive the anisotropy write"
        );

        for (name, value) in [
            (SSAO_SLICES_KEY, Value::Int(2)),
            (SSAO_BLUR_PASSES_KEY, Value::Int(1)),
            (SSAO_BENT_NORMALS_KEY, Value::Bool(false)),
        ] {
            assert_eq!(
                apply(
                    &mut stack,
                    &format!("{VIDEO_NAMESPACE}.{name}"),
                    &value,
                    &mut stage,
                ),
                Ok(Applied::Live),
            );
        }
        let video = *stage.video.last().expect("the renderer was told");
        assert_eq!(
            (
                video.ssao_slices,
                video.ssao_blur_passes,
                video.ssao_bent_normals
            ),
            (2, 1, false),
        );

        let key = format!("{AUDIO_NAMESPACE}.{}", Bus::Music.settings_key());
        assert_eq!(
            apply(&mut stack, &key, &Value::Float(0.25), &mut stage),
            Ok(Applied::Live)
        );
        assert_eq!(stage.gains, [(Bus::Music, 0.25)]);
    }

    /// **A host with no seam still writes the key**, and says the next start-up
    /// is where it lands.
    ///
    /// [`Unsupported`] is not a refusal — `apps/options` has no renderer and
    /// still has to write every video row — and the distinction is what its
    /// "next start" mark on the row means.
    #[test]
    fn a_host_with_no_seam_writes_the_key_and_says_next_start() {
        let key = format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}");
        let mut stack = stack_from("");
        // Every method left at its default, which is the whole of what a bundle
        // with no renderer answers.
        struct Nowhere;
        impl Stage for Nowhere {}

        assert_eq!(
            apply(&mut stack, &key, &Value::Float(0.5), &mut Nowhere),
            Ok(Applied::NextStart)
        );
        assert!(
            (render_scale(&stack) - 0.5).abs() < f32::EPSILON,
            "the key was not written: {}",
            render_scale(&stack)
        );
    }
}
