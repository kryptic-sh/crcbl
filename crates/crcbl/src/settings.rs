//! What the player's settings file says, for the two engine layers that read
//! one: `[engine.video]` and `[engine.audio]`.
//!
//! # `[engine.video]`: one clamp in a chain of four
//!
//! Which of topic 18's effects the **player** allows, in
//! topic 39's effect resolution order:
//!
//! ```text
//! camera stack declares what the view wants
//!   → [engine.video] clamps it downward as a player quality setting   ← here
//!   → programmatic override may set it either way
//!   → device capability clamps it downward, last and absolutely
//! ```
//!
//! # `[engine.audio]`: two layers, and the key **is** the value
//!
//! A bus gain resolves through the player's file and the game's programmatic
//! control, and that is all — the audio rules in `docs/notes/simulation.md`
//! spell out why the other two layers are missing rather than unbuilt. There is no per-camera layer
//! because there is one listener and one mix; there is no device-capability
//! layer because no audio device removes the ability to multiply a sample by a
//! scalar.
//!
//! **So an audio key is unlike a video key in the direction it may move.**
//! `[engine.video]` may only clamp downward and an absent key clamps nothing;
//! an `[engine.audio]` key _is_ the gain, and there is nothing above it for it
//! to clamp against. An absent key is unity — a player who has said nothing
//! about the music has not asked for it to be quieter.
//!
//! [`crcbl_store::settings`] is the mechanism — layered TOML, dotted keys,
//! typed reads — and [`crcbl_render::effects`] is the resolution point. This
//! module is the join between them, and it is here rather than in either
//! because neither may depend on the other: `crcbl-render` has no storage and
//! `crcbl-store` has no idea what an effect is.
//!
//! # Where the read happens
//!
//! [`GpuContext::open`](crate::engine::GpuContext::open) and its two siblings,
//! from [`SettingsSource`](crate::engine::SettingsSource) — so every sample and
//! every `crcbl new` scaffold reads the player's settings without asking, and
//! [`GpuContext::effect_request`](crate::engine::GpuContext::effect_request) is
//! what a renderer built on that context is handed. Nothing in that path is
//! fallible: a player with no settings file is the ordinary first run, not a
//! start-up that failed.
//!
//! # A key that is absent is not a key that says "off"
//!
//! This layer only ever **clamps downward**, so the question each key answers
//! is "has the player asked for *less*?" — and a file that does not mention an
//! effect has not. [`video_effects`] therefore starts from
//! [`RenderEffects::all`] and removes a bit only for a key that is present and
//! `false`; `true` and absent are the same answer, which is why a settings file
//! that says nothing cannot switch a frame's passes off.
//!
//! # A video key need not be a boolean, and [`render_scale`] is the first that
//! is not
//!
//! The clamp-downward rule survives the change of type rather than being an
//! exception to it: a render scale below one draws fewer pixels than the game
//! asked for, and a value above one is clamped to one rather than asked for. So
//! the key still only ever takes away, an absent key still takes nothing, and
//! the range is the one
//! [`ForwardRenderer::set_render_scale`](crcbl_render::ForwardRenderer::set_render_scale)
//! already enforces — this reader clamps to the same bounds rather than
//! trusting the file, because the two must not be able to disagree.
//!
//! # And [`frame_limit`] is the first key that clamps something it cannot see
//!
//! Every key above resolves to a value on its own: a bit is on or off, a scale
//! is a fraction of the extent. A frame-rate ceiling is not — "less" means less
//! than whatever the *game* asked for, which is a runtime value no reader here
//! holds. So this one answers with the ceiling and leaves the comparison to
//! [`FrameLimit::clamped_to`], where the ordering that makes it work — unlimited
//! being above every rate rather than below it, though it is spelled zero —
//! belongs to the type rather than to the file.
//!
//! # And [`antialiasing`] is the first key that **replaces** rather than clamps
//!
//! Every key above answers "has the player asked for less?", and the answer only
//! ever removes. The antialiasing tier cannot: the frame has one resolve slot,
//! and a player who picked CMAA2 where the camera asked for FXAA has asked for a
//! *different* filter rather than a smaller one — an intersection of the two
//! leaves neither, and a union runs both. So the key holds a
//! [`Antialiasing`] rung by name,
//! [`EffectRequest::antialiasing`](crcbl_render::EffectRequest::antialiasing)
//! carries it, and
//! [`EffectRequest::resolve`](crcbl_render::EffectRequest::resolve) applies it
//! as a replacement inside that slot.
//! An absent key is still "the player has said nothing", which here means the
//! view's own stack keeps the tier it asked for.
//!
//! **`antialiasing` used to be a boolean and `smaa` used to be a key.** Both are
//! gone, and so is the tier the second was named for; [`antialiasing`]'s docs
//! say what a file still holding either old spelling reads as.
//!
//! # And [`anisotropic_filtering`] is the first key that may ask for more
//!
//! Every key above answers at most what the game asked for. This one runs from
//! `1`, which turns the filter off, to [`MAX_ANISOTROPIC_FILTERING`], which is
//! twice the engine's [`DEFAULT_ANISOTROPY`] — so a file can ask for *more*
//! work than a run with no file does. It is allowed because the spend is
//! bounded by the device and by nothing else: the seam's ceiling is
//! `Limits::max_sampler_anisotropy`, and
//! [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy)
//! clamps to it, one on a device without the feature. This reader clamps to
//! the range a file may spell and the setter finishes the job, which is why
//! the file holds the player's ask rather than one machine's answer to it —
//! the ask follows them to a machine with a different ceiling. An absent key is
//! still the engine's own figure, as it is for every key here.
//!
//! # A quality tier is a writer over these keys, not a key of its own
//!
//! [`presets`] is topic 39's tier table: selecting `low`,
//! `medium` or `high` writes the individual keys that column names, through
//! [`apply`], and nothing in the resolution order ever consults the tier. So it
//! is a console command rather than a [`CatalogueKey`], and the word for which
//! tier a file is on is derived from the readers above rather than stored —
//! that module's header says why, and why `custom` is what a file that is on no
//! tier reads as.

#[cfg(doc)]
use crcbl_render::{Antialiasing, DEFAULT_ANISOTROPY, RenderEffects};

#[cfg(doc)]
use crate::engine::FrameLimit;

mod console;
mod engine_audio;
mod engine_video;
mod key_catalogue;
pub mod presets;
mod renderer;
mod stage;

pub use console::{ConsoleHost, SharedSettings, console_bindings, dump, save};
pub use engine_audio::{AUDIO_NAMESPACE, audio_gains, set_audio_gain};
pub use engine_video::{
    ANISOTROPIC_FILTERING_KEY, ANTIALIASING_KEY, ANTIALIASING_NAMES, FRAME_LIMIT_CEILING,
    FRAME_LIMIT_KEY, MAX_ANISOTROPIC_FILTERING, MAX_UI_SCALE, MIN_UI_SCALE, RENDER_SCALE_KEY,
    SHADOW_FILTER_KEY, SHADOW_FILTER_NAMES, SSAO_BENT_NORMALS_KEY, SSAO_BLUR_PASSES_KEY,
    SSAO_SLICES_KEY, TIER_VIDEO_KEYS, UI_SCALE_KEY, VIDEO_KEYS, VIDEO_NAMESPACE, VideoSettings,
    anisotropic_filtering, antialiasing, frame_limit, render_scale, set_anisotropic_filtering,
    set_antialiasing, set_frame_limit, set_render_scale, set_shadow_filter, set_ssao_bent_normals,
    set_ssao_blur_passes, set_ssao_slices, set_ui_scale, set_video, set_video_effects,
    shadow_filter, ssao_bent_normals, ssao_blur_passes, ssao_slices, ui_scale, video,
    video_effects,
};
pub use key_catalogue::{CatalogueKey, KeyStatus, catalogue, catalogued};
pub(crate) use renderer::apply_process_video;
#[cfg(test)]
pub(crate) use renderer::process_video_test_guard;
pub use renderer::{apply_video_to, debug_view_switches, set_debug_view_on};
pub use stage::{Applied, Deferred, GpuStage, Stage, Unsupported, apply};

// Re-imported for `presets`, which reads the same tier fallback the console's
// row reads.
use engine_video::antialiasing_or_default;

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_audio::mixer::Bus;
    use crcbl_store::MemoryStorage;
    use crcbl_store::StorageSource;
    use crcbl_store::settings::{SETTINGS_FILE, SettingsStack};

    use crate::engine::FrameLimit;

    /// A stack over `storage`, edited by `edit`, saved back, and reloaded
    /// through the real loader.
    ///
    /// **The round trip is the point.** A writer that serialises into an
    /// in-memory table proves nothing about the file: the section header, the
    /// TOML type each key lands as, and whether `save` ever ran are all between
    /// the two halves, and every one of them is a way for a settings screen to
    /// keep a value the next start-up will not read.
    pub(super) fn round_trip(edit: impl FnOnce(&mut SettingsStack)) -> (SettingsStack, String) {
        let storage = MemoryStorage::new();
        let path = std::path::Path::new(SETTINGS_FILE);
        let mut stack = SettingsStack::from_storage(&storage);
        edit(&mut stack);
        stack.save(&storage, path).expect("memory storage saves");
        let written = String::from_utf8(storage.read(path).expect("the save wrote a file"))
            .expect("the writer emits UTF-8");
        (SettingsStack::from_storage(&storage), written)
    }

    /// **A value arithmetic cannot use is refused, not written.**
    ///
    /// The readers warn about a `nan` a player hand-edited in; a caller handing
    /// one to a writer has a bug, and a file holding it would look deliberate
    /// to whoever read it next.
    #[test]
    fn a_scale_or_a_gain_that_is_not_a_number_is_refused() {
        let storage = MemoryStorage::new();
        let mut stack = SettingsStack::from_storage(&storage);
        for bad in [f32::NAN, f32::INFINITY] {
            assert!(
                set_render_scale(&mut stack, bad).is_err(),
                "a render scale of {bad} was accepted"
            );
            assert!(
                set_anisotropic_filtering(&mut stack, bad).is_err(),
                "an anisotropy of {bad} was accepted"
            );
            assert!(
                set_audio_gain(&mut stack, Bus::Music, bad).is_err(),
                "a music gain of {bad} was accepted"
            );
        }
        assert!(
            !stack.contains(&format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}")),
            "a refused write still put the key in the stack"
        );
    }

    /// A stack over a settings file with `toml` in it, through the real
    /// loader — not a table built in memory, because the spelling of the
    /// section header is half of what this module has to get right.
    pub(super) fn stack_from(toml: &str) -> SettingsStack {
        let storage = MemoryStorage::new();
        storage
            .write(std::path::Path::new(SETTINGS_FILE), toml.as_bytes())
            .expect("memory storage accepts every write");
        SettingsStack::from_storage(&storage)
    }

    /// A [`Stage`] that records what it was asked to apply, and answers
    /// [`Unsupported`] for the seam it does not have.
    ///
    /// A recorder rather than a real bundle because what these tests are about
    /// is what [`apply`] *asked for*: the renderer's own arithmetic is
    /// `crcbl-render`'s and needs a device, and asserting on it here would be
    /// asserting on the wrong half.
    #[derive(Debug, Default)]
    pub(super) struct Recorder {
        pub(super) video: Vec<VideoSettings>,
        pub(super) gains: Vec<(Bus, f32)>,
        pub(super) limits: Vec<FrameLimit>,
        pub(super) ui_scales: Vec<f32>,
        /// Whether `set_frame_limit` has a clock behind it, so one test can turn
        /// the seam off and read [`Applied::NextStart`] back.
        pub(super) has_clock: bool,
    }

    impl Stage for Recorder {
        fn apply_video(&mut self, video: &VideoSettings) -> Result<(), Unsupported> {
            self.video.push(*video);
            Ok(())
        }

        fn set_bus_gain(&mut self, bus: Bus, gain: f32) -> Result<(), Unsupported> {
            self.gains.push((bus, gain));
            Ok(())
        }

        fn set_frame_limit(&mut self, limit: FrameLimit) -> Result<(), Unsupported> {
            if !self.has_clock {
                return Err(Unsupported);
            }
            self.limits.push(limit);
            Ok(())
        }

        fn set_ui_scale(&mut self, scale: f32) -> Result<(), Unsupported> {
            self.ui_scales.push(scale);
            Ok(())
        }
    }
}
