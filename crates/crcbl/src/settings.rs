//! What the player's settings file says, for the two engine layers that read
//! one: `[engine.video]` and `[engine.audio]`.
//!
//! # `[engine.video]`: one clamp in a chain of four
//!
//! Which of topic 18's effects the **player** allows, in
//! `docs/plan/39-capabilities.md`'s effect resolution order:
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
//! control, and that is all — `docs/plan/13-audio.md` spells out why the other
//! two layers are missing rather than unbuilt. There is no per-camera layer
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
//! and a player who picked SMAA where the camera asked for FXAA has asked for a
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
//! gone; [`antialiasing`]'s docs say what a file still holding the old spelling
//! reads as.
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

use std::any::Any;

use crcbl_audio::mixer::Bus;
use crcbl_console::{Binding, Fault, Flags, Kind, Value};
use crcbl_render::{Antialiasing, DEFAULT_ANISOTROPY, MIN_RENDER_SCALE, RenderEffects};

use crate::engine::FrameLimit;
use crcbl_store::StorageError;
use crcbl_store::settings::SettingsStack;

/// The `[engine.video]` section, as a dotted key prefix.
pub const VIDEO_NAMESPACE: &str = "engine.video";

/// Every effect a player can switch off with a boolean, and the
/// `[engine.video]` key that does it.
///
/// **The one place such a key is spelled.** A settings screen writing the row
/// and a start-up reading it back go through this table, because two spellings
/// of one key is a game that saves a setting it will never load again.
///
/// The names are the ones `crcbl_store::settings`' own examples already use for
/// this namespace — bare snake_case nouns beside `vsync` and `master_volume` —
/// rather than the flag spellings (`no_shadows`, `enable_ssao`) the same
/// switches have on a command line. A settings file is a description of what
/// the player wants on, and a negated key would make `shadows = false` and
/// `no_shadows = false` both writable and opposite.
///
/// **The two antialiasing bits are deliberately not here.** They share one
/// resolve slot, so a pair of booleans is a panel that can switch both on and a
/// frame that then picks between them out of sight;
/// [`ANTIALIASING_KEY`] is the ladder that replaced them and
/// [`antialiasing`] is its reader.
pub const VIDEO_KEYS: [(&str, RenderEffects); 6] = [
    ("shadows", RenderEffects::SHADOWS),
    ("ambient_occlusion", RenderEffects::AMBIENT_OCCLUSION),
    ("reflections", RenderEffects::REFLECTIONS),
    ("bloom", RenderEffects::BLOOM),
    ("volumetric_fog", RenderEffects::VOLUMETRIC_FOG),
    ("auto_exposure", RenderEffects::AUTO_EXPOSURE),
];

/// What the player's `[engine.video]` section allows, for
/// [`EffectRequest::video`](crcbl_render::EffectRequest::video).
///
/// [`RenderEffects::all`] for a stack that says nothing, and one bit fewer for
/// each key present and `false`.
///
/// # A line that does nothing says so
///
/// A key holding something that is not a boolean — `shadows = "off"` in a
/// hand-edited file — leaves its effect standing, which is
/// [`SettingsStack::get`]'s own rule for a value it cannot deserialize and the
/// safe direction for a layer that may only remove. It also **warns**, naming
/// the key: silence there is a player who wrote a line, saw no change, and has
/// nothing to read that would tell them why. A key that is simply absent is not
/// a mistake and does not warn.
#[must_use]
pub fn video_effects(stack: &SettingsStack) -> RenderEffects {
    let mut allowed = RenderEffects::all();
    for (key, effect) in VIDEO_KEYS {
        let dotted = format!("{VIDEO_NAMESPACE}.{key}");
        match stack.get::<bool>(&dotted) {
            Some(false) => allowed.remove(effect),
            Some(true) => {}
            // Present, and not something this layer can read. `get` has already
            // searched every layer for a `bool`, so nothing below answered
            // either.
            None if stack.contains(&dotted) => crcbl_core::log::warn!(
                "settings: `{dotted}` is not true or false, so it does nothing; \
                 the effect stays as the game asked for it"
            ),
            None => {}
        }
    }
    allowed
}

/// The `[engine.video]` key that sizes the renderer's internal target.
///
/// Spelled here for [`VIDEO_KEYS`]' reason and not put *in* that table: the
/// table pairs a key with the [`RenderEffects`] bit it clears, and this key
/// clears no bit. A settings screen writing the row and a start-up reading it
/// back still go through one spelling.
pub const RENDER_SCALE_KEY: &str = "render_scale";

/// The `[engine.video]` key that caps the loop's frame rate.
///
/// Spelled here for [`RENDER_SCALE_KEY`]'s reason, and read by
/// [`frame_limit`].
pub const FRAME_LIMIT_KEY: &str = "frame_limit";

/// The highest rate [`FRAME_LIMIT_KEY`] can hold: the ceiling of the type
/// [`frame_limit`] reads it as.
///
/// [`FrameLimit`] is a `u32` of frames a second, so this is that type's own
/// ceiling widened to the `i64` a [`Kind::Int`] range is spelled in — not a
/// rate anyone will ask for, and not a number this file gets to choose either.
/// A file above it is what [`frame_limit`] already warns about.
pub const FRAME_LIMIT_CEILING: i64 = u32::MAX as i64;

/// The `[engine.video]` key that picks the frame's antialiasing tier.
///
/// Spelled here for [`RENDER_SCALE_KEY`]'s reason — it clears no
/// [`RenderEffects`] bit on its own, it *replaces* the pair of them that make
/// the resolve slot — and read by [`antialiasing`].
pub const ANTIALIASING_KEY: &str = "antialiasing";

/// Every rung [`antialiasing`] reads, as the words a file and a console line
/// spell them with — the [`Kind::Enum`] set of [`ANTIALIASING_KEY`].
///
/// **Derived from [`Antialiasing::ALL`] rather than written out**, through
/// [`Antialiasing::name`], which is already "the one place a rung's spelling is
/// written". A literal list here would be a second spelling of every rung and a
/// silent omission of the next one; this cannot be either. The `while` loop is
/// what a `const` context has instead of `map`.
pub const ANTIALIASING_NAMES: [&str; Antialiasing::ALL.len()] = {
    let mut names = [""; Antialiasing::ALL.len()];
    let mut i = 0;
    while i < Antialiasing::ALL.len() {
        names[i] = Antialiasing::ALL[i].name();
        i += 1;
    }
    names
};

/// The `[engine.video]` key that sets the base-colour page's anisotropy.
///
/// Spelled here for [`RENDER_SCALE_KEY`]'s reason, and read by
/// [`anisotropic_filtering`].
pub const ANISOTROPIC_FILTERING_KEY: &str = "anisotropic_filtering";

/// The most [`anisotropic_filtering`] reads: the desktop ceiling.
///
/// [`Limits::desktop`](crcbl_hal::Limits::desktop)'s figure rather than a
/// number of this file's, so the key's top and the seam's desktop preset
/// cannot drift apart. A device whose own ceiling is lower — and one without
/// `SAMPLER_ANISOTROPY`, whose ceiling is one — is
/// [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy)'s
/// clamp, not this one; see the [module docs](self).
pub const MAX_ANISOTROPIC_FILTERING: f32 = crcbl_hal::Limits::desktop().max_sampler_anisotropy;

/// Everything the player's `[engine.video]` section says, read in one pass.
///
/// One type rather than a reader per key because a caller wants all of it at
/// the same moment — [`GpuContext::open`](crate::engine::GpuContext::open)
/// reads the section once while it is opening — and because building a
/// [`SettingsStack`] per key would read the player's file once per key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoSettings {
    /// Which of topic 18's effects the player allows; see [`video_effects`].
    pub effects: RenderEffects,
    /// Which antialiasing tier the player picked, or [`None`] for a player who
    /// picked none; see [`antialiasing`].
    ///
    /// **Not a bit in [`effects`](Self::effects)**, because it replaces the
    /// resolve slot rather than clamping it — see the [module docs](self).
    pub antialiasing: Option<Antialiasing>,
    /// What fraction of the caller's extent the renderer draws at; see
    /// [`render_scale`].
    pub render_scale: f32,
    /// The anisotropy the base-colour page is sampled with; see
    /// [`anisotropic_filtering`].
    pub anisotropic_filtering: f32,
    /// The ceiling the player puts on the loop's frame rate; see
    /// [`frame_limit`].
    ///
    /// A **ceiling**, not the rate the loop runs at:
    /// [`FrameLimit::clamped_to`] is what a caller holding the game's own limit
    /// applies it with, and [`FrameLimit::unlimited`] is the ceiling that holds
    /// nothing down.
    pub frame_limit: FrameLimit,
}

impl VideoSettings {
    /// What a player who has said nothing gets: every effect standing, no
    /// antialiasing tier picked, the full extent and the page at the engine's
    /// own anisotropy.
    ///
    /// Also what [`SettingsSource::None`](crate::engine::SettingsSource::None)
    /// answers, and the two are the same answer for the same reason — this
    /// layer may only take away, so "nothing to read" and "nothing taken away"
    /// cannot differ.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            effects: RenderEffects::all(),
            antialiasing: None,
            render_scale: 1.0,
            anisotropic_filtering: DEFAULT_ANISOTROPY,
            frame_limit: FrameLimit::unlimited(),
        }
    }
}

/// The whole `[engine.video]` section, off one stack.
#[must_use]
pub fn video(stack: &SettingsStack) -> VideoSettings {
    VideoSettings {
        effects: video_effects(stack),
        antialiasing: antialiasing(stack),
        render_scale: render_scale(stack),
        anisotropic_filtering: anisotropic_filtering(stack),
        frame_limit: frame_limit(stack),
    }
}

/// Which antialiasing tier the player picked, for
/// [`EffectRequest::antialiasing`](crcbl_render::EffectRequest::antialiasing).
///
/// [`None`] for a stack that says nothing, which leaves the view's own stack
/// holding the resolve slot, and otherwise the [`Antialiasing`] rung the key
/// names — `"none"`, `"fxaa"` or `"smaa"`, [`Antialiasing::name`]'s spelling on
/// both sides of the round trip.
///
/// # A file still holding the boolean reads as one of two things
///
/// The key was a `bool` beside a second key called `smaa` until
/// `docs/plan/49-antialiasing.md`'s eighth decision folded the pair into this
/// ladder. There is no migration — everything here is v0 — but the two spellings
/// a hand-edited file can still hold are answered rather than warned about,
/// because both had a meaning and neither is a mistake the player made:
/// `antialiasing = true` was "the player has not asked for less", which is
/// exactly [`None`] here, and `antialiasing = false` was "no resolve at all",
/// which is [`Antialiasing::None`]. A `smaa` key is not read by anything and is
/// reported by `crcbl settings list` as a key the engine does not define.
///
/// # A line that does nothing says so
///
/// Any other value — a number, or a word no rung wears — leaves the tier
/// unpicked and **warns**, naming the key, on [`video_effects`]' terms. A key
/// that is simply absent is not a mistake and does not warn.
#[must_use]
pub fn antialiasing(stack: &SettingsStack) -> Option<Antialiasing> {
    let dotted = format!("{VIDEO_NAMESPACE}.{ANTIALIASING_KEY}");
    let tier = match stack.get::<String>(&dotted) {
        Some(name) => Antialiasing::from_name(&name),
        // Not a string, so it may be the boolean this key used to be. Both of
        // its values are answered rather than warned about, on the terms above.
        None => match stack.get::<bool>(&dotted) {
            Some(true) => return None,
            Some(false) => return Some(Antialiasing::None),
            _ => None,
        },
    };
    if tier.is_none() && stack.contains(&dotted) {
        crcbl_core::log::warn!(
            "settings: `{dotted}` names no antialiasing tier, so it does nothing; \
             the frame is resolved the way the game asked for it"
        );
    }
    tier
}

/// What fraction of the caller's extent the player wants drawn, for
/// [`ForwardRenderer::set_render_scale`](crcbl_render::ForwardRenderer::set_render_scale).
///
/// `1.0` for a stack that says nothing, and otherwise the file's value clamped
/// to `MIN_RENDER_SCALE..=1.0` — the same bounds the setter enforces, so a file
/// asking for a tenth and a file asking for a quarter produce the same frame
/// and neither produces a target the renderer refused to size.
///
/// # A line that does nothing says so
///
/// A key holding something this cannot use — `render_scale = "half"`, and also
/// `nan` and `inf`, which TOML spells and arithmetic cannot — leaves the scale
/// at `1.0` and **warns**, naming the key, on [`video_effects`]' terms. A
/// finite value outside the range does not warn: it is a number the player
/// meant, and clamping it is this layer's job rather than a mistake to report.
#[must_use]
pub fn render_scale(stack: &SettingsStack) -> f32 {
    let dotted = format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}");
    let unreadable = || {
        crcbl_core::log::warn!(
            "settings: `{dotted}` is not a usable number, so it does nothing; \
             the frame is drawn at the extent the game asked for"
        );
        1.0
    };
    match stack.get::<f64>(&dotted) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "clamped to [MIN_RENDER_SCALE, 1.0], where every f64 has an f32 within an ulp"
        )]
        // `clamp` answers NaN for NaN rather than a bound, so the finite check
        // has to come first — a scale of NaN reaches `begin_frame` as an extent
        // of zero pixels, where a nonsense string reaches it as a full frame.
        Some(scale) if scale.is_finite() => (scale as f32).clamp(MIN_RENDER_SCALE, 1.0),
        Some(_) => unreadable(),
        None if stack.contains(&dotted) => unreadable(),
        None => 1.0,
    }
}

/// The anisotropy the player wants the base-colour page sampled with, for
/// [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy).
///
/// [`DEFAULT_ANISOTROPY`] for a stack that says nothing, and otherwise the
/// file's value clamped to `1.0..=MAX_ANISOTROPIC_FILTERING`. The lower bound
/// is the value that turns the filter off; the upper is the desktop ceiling,
/// and a device that offers less — or none — is the setter's clamp, so a file
/// asking for sixteen on such a device gets what it has rather than a sampler
/// it refuses. The [module docs](self) say why this is the one key that may
/// ask for more than the engine's default.
///
/// # A line that does nothing says so
///
/// On [`render_scale`]'s terms exactly: a value this cannot use — a string,
/// `nan`, `inf` — leaves the default and **warns**, naming the key, and a
/// finite value outside the range is clamped without a word.
#[must_use]
pub fn anisotropic_filtering(stack: &SettingsStack) -> f32 {
    let dotted = format!("{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}");
    let unreadable = || {
        crcbl_core::log::warn!(
            "settings: `{dotted}` is not a usable number, so it does nothing; \
             the page is sampled at the engine's default anisotropy"
        );
        DEFAULT_ANISOTROPY
    };
    match stack.get::<f64>(&dotted) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "clamped to [1, MAX_ANISOTROPIC_FILTERING], where every f64 has an f32 within an ulp"
        )]
        // The finite check first, for `render_scale`'s reason: `clamp` answers
        // NaN for NaN, and a NaN is the one value `set_anisotropy` reads as
        // "the default" — which is right, and would hide that the file said
        // something unusable.
        Some(anisotropy) if anisotropy.is_finite() => {
            (anisotropy as f32).clamp(1.0, MAX_ANISOTROPIC_FILTERING)
        }
        Some(_) => unreadable(),
        None if stack.contains(&dotted) => unreadable(),
        None => DEFAULT_ANISOTROPY,
    }
}

/// The ceiling the player puts on the loop's frame rate, for
/// [`FrameLimit::clamped_to`].
///
/// [`FrameLimit::unlimited`] for a stack that says nothing, and otherwise the
/// file's value. **Zero reads as unlimited and is not a mistake** — it is the
/// spelling [`FrameLimit::fps`] already gives "no cap", and here it means the
/// same thing an absent key does: this layer may only clamp downward, and a
/// player who asked for no ceiling has asked for nothing to be taken away.
///
/// The value is a ceiling rather than the rate the loop runs at, so a game
/// already capped below it keeps its own cap. That is what
/// [`FrameLimit::clamped_to`] does with the two, and why nothing here compares
/// the rates itself.
///
/// # A line that does nothing says so
///
/// A key holding something this cannot use — `frame_limit = "sixty"`, a
/// negative, or a rate past [`u32::MAX`] — leaves the ceiling unlimited and
/// **warns**, naming the key, on [`video_effects`]' terms.
#[must_use]
pub fn frame_limit(stack: &SettingsStack) -> FrameLimit {
    let dotted = format!("{VIDEO_NAMESPACE}.{FRAME_LIMIT_KEY}");
    match stack.get::<u32>(&dotted) {
        Some(fps) => FrameLimit::fps(fps),
        None if stack.contains(&dotted) => {
            crcbl_core::log::warn!(
                "settings: `{dotted}` is not a usable frame rate, so it does nothing; \
                 the loop runs at the limit the game asked for"
            );
            FrameLimit::unlimited()
        }
        None => FrameLimit::unlimited(),
    }
}

/// Write the whole `[engine.video]` section into the stack's user layer.
///
/// The mirror of [`video`], key for key: every entry of [`VIDEO_KEYS`],
/// [`ANTIALIASING_KEY`], [`RENDER_SCALE_KEY`], [`ANISOTROPIC_FILTERING_KEY`] and
/// [`FRAME_LIMIT_KEY`]. Nothing is persisted until the
/// caller saves the stack
/// — see [`SettingsStack::save_platform`].
///
/// # It writes the row that says "on", where the reader ignores it
///
/// The reader treats `true` and absent alike, because this layer may only clamp
/// downward. A writer cannot: a settings screen that only ever wrote `false`
/// could never turn an effect back *on*, since removing the key and writing
/// `true` differ to a file and not to the reader. So every key in the table is
/// written on every call, which also means the file a settings screen produces
/// says what the player chose rather than only where they differed from the
/// engine.
///
/// # The antialiasing tier is the one key it may leave out
///
/// Its domain has no word for "unpicked": `"none"` is a tier — the one that
/// draws no resolve at all — so the only way a file says the player picked
/// nothing is by not holding the key. A [`None`] therefore writes no row and
/// **leaves any row already there standing**, which is the one case this writer
/// cannot express; a screen that wants the key gone rewrites it with a tier.
///
/// # Errors
///
/// [`SettingsStack::set`]'s: no user layer in the stack, or an ancestor of a
/// key already holding a scalar in a hand-edited file.
pub fn set_video(stack: &mut SettingsStack, video: VideoSettings) -> Result<(), StorageError> {
    set_video_effects(stack, video.effects)?;
    if let Some(tier) = video.antialiasing {
        set_antialiasing(stack, tier)?;
    }
    set_render_scale(stack, video.render_scale)?;
    set_anisotropic_filtering(stack, video.anisotropic_filtering)?;
    set_frame_limit(stack, video.frame_limit)
}

/// Write `[engine.video] antialiasing`, as the rung [`antialiasing`] reads back.
///
/// The tier is written by [`Antialiasing::name`], which is the reader's spelling
/// too — a settings screen and a start-up disagreeing about the word for a rung
/// is a filter the player picks once and never sees.
///
/// It takes a tier rather than an [`Option`] for [`set_video`]'s reason: there
/// is no word for "unpicked", so a writer's only choice is which rung to name.
///
/// # Errors
///
/// [`set_video`]'s.
pub fn set_antialiasing(stack: &mut SettingsStack, tier: Antialiasing) -> Result<(), StorageError> {
    stack.set(
        &format!("{VIDEO_NAMESPACE}.{ANTIALIASING_KEY}"),
        &tier.name(),
    )
}

/// Write the effect rows of `[engine.video]`, one key per [`VIDEO_KEYS`] entry.
///
/// # Errors
///
/// [`set_video`]'s.
pub fn set_video_effects(
    stack: &mut SettingsStack,
    allowed: RenderEffects,
) -> Result<(), StorageError> {
    for (key, effect) in VIDEO_KEYS {
        stack.set(
            &format!("{VIDEO_NAMESPACE}.{key}"),
            &allowed.contains(effect),
        )?;
    }
    Ok(())
}

/// Write `[engine.video] render_scale`, clamped to what [`render_scale`] reads.
///
/// **Clamped on the way in as well as on the way out**, so the file holds the
/// scale the next start-up will actually draw at. A settings screen that stored
/// a slider's raw 0.1 and read back 0.25 would show the player a control that
/// jumps under their finger on the next launch, and the range is
/// `MIN_RENDER_SCALE..=1.0` in both directions because
/// [`ForwardRenderer::set_render_scale`](crcbl_render::ForwardRenderer::set_render_scale)
/// is the one enforcing it.
///
/// # Errors
///
/// [`set_video`]'s, and a `scale` that is not finite: the readers warn about a
/// `nan` in a hand-edited file because a player put it there, but a caller
/// handing one to a writer has a bug, and writing `1.0` on its behalf would
/// hide it in a file that then looks deliberate.
pub fn set_render_scale(stack: &mut SettingsStack, scale: f32) -> Result<(), StorageError> {
    let dotted = format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}");
    if !scale.is_finite() {
        return Err(StorageError::Other(format!(
            "settings: `{dotted}` cannot be written as {scale}"
        )));
    }
    stack.set(&dotted, &f64::from(scale.clamp(MIN_RENDER_SCALE, 1.0)))
}

/// Write `[engine.video] anisotropic_filtering`, clamped to what
/// [`anisotropic_filtering`] reads.
///
/// Clamped on the way in on [`set_render_scale`]'s terms, and to this layer's
/// range rather than the device's: the file is the player's ask, which follows
/// them to a machine with a different ceiling, and
/// [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy)
/// clamps to the device at the moment it matters.
///
/// # Errors
///
/// [`set_video`]'s, and a value that is not finite, for [`set_render_scale`]'s
/// reason.
pub fn set_anisotropic_filtering(
    stack: &mut SettingsStack,
    anisotropy: f32,
) -> Result<(), StorageError> {
    let dotted = format!("{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}");
    if !anisotropy.is_finite() {
        return Err(StorageError::Other(format!(
            "settings: `{dotted}` cannot be written as {anisotropy}"
        )));
    }
    stack.set(
        &dotted,
        &f64::from(anisotropy.clamp(1.0, MAX_ANISOTROPIC_FILTERING)),
    )
}

/// Write `[engine.video] frame_limit`, as the rate [`frame_limit`] reads back.
///
/// **[`FrameLimit::unlimited`] is written as `0` rather than left out**, which
/// is the same choice [`set_video_effects`] makes for a `true`: the two differ
/// to a file and not to the reader, and a settings screen that omitted the row
/// could never move a player's ceiling back off a cap they had saved.
///
/// # Errors
///
/// [`set_video`]'s.
pub fn set_frame_limit(stack: &mut SettingsStack, limit: FrameLimit) -> Result<(), StorageError> {
    stack.set(
        &format!("{VIDEO_NAMESPACE}.{FRAME_LIMIT_KEY}"),
        &limit.rate(),
    )
}

/// The `[engine.audio]` section, as a dotted key prefix.
pub const AUDIO_NAMESPACE: &str = "engine.audio";

/// Every bus gain the player's file can set, paired with the bus it moves.
///
/// Unity for a bus the file says nothing about, and the file's value clamped to
/// `[0, 1]` for one it does. The order is [`Bus::ALL`]'s, so a caller can hand
/// the whole thing to
/// [`Mixer::set_bus_gain`](crcbl_audio::mixer::Mixer::set_bus_gain) in a loop
/// without knowing which buses exist.
///
/// # A line that does nothing says so
///
/// A key holding something this cannot read as a number — `music_volume =
/// "half"` — leaves its bus at unity and **warns**, naming the key, on
/// [`video_effects`]' terms: a player who wrote a line, heard no change, and has
/// nothing to read that would tell them why is the failure worth a log line. A
/// key that is simply absent is not a mistake and does not warn.
#[must_use]
pub fn audio_gains(stack: &SettingsStack) -> [(Bus, f32); Bus::ALL.len()] {
    Bus::ALL.map(|bus| {
        let dotted = format!("{AUDIO_NAMESPACE}.{}", bus.settings_key());
        let unreadable = || {
            crcbl_core::log::warn!(
                "settings: `{dotted}` is not a usable number, so it does nothing; \
                 the {bus:?} bus stays where the game set it"
            );
            (bus, 1.0)
        };
        match stack.get::<f64>(&dotted) {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a gain is clamped to [0, 1], where every f64 has an f32 within an ulp"
            )]
            // Finite first: `clamp` answers NaN for NaN, and a NaN gain
            // multiplies every sample of the bus into silence-shaped garbage.
            Some(gain) if gain.is_finite() => (bus, gain.clamp(0.0, 1.0) as f32),
            Some(_) => unreadable(),
            None if stack.contains(&dotted) => unreadable(),
            None => (bus, 1.0),
        }
    })
}

/// Write one bus's gain into `[engine.audio]`, clamped to what
/// [`audio_gains`] reads.
///
/// The bus is spelled by [`Bus::settings_key`], which is the reader's spelling
/// too — a settings screen and a start-up disagreeing about the name of the
/// music slider is a volume the player sets once and never hears again.
///
/// One bus rather than the whole array, unlike [`set_video`]: an audio key
/// **is** the gain, so a bus the file does not mention is already at unity and
/// there is no row a writer has to state to mean "on".
///
/// # Errors
///
/// [`set_video`]'s, and a `gain` that is not finite, for [`set_render_scale`]'s
/// reason.
pub fn set_audio_gain(stack: &mut SettingsStack, bus: Bus, gain: f32) -> Result<(), StorageError> {
    let dotted = format!("{AUDIO_NAMESPACE}.{}", bus.settings_key());
    if !gain.is_finite() {
        return Err(StorageError::Other(format!(
            "settings: `{dotted}` cannot be written as {gain}"
        )));
    }
    stack.set(&dotted, &f64::from(gain.clamp(0.0, 1.0)))
}

// ── The catalogue ───────────────────────────────────────────────────────────

/// Whether anything in this workspace reads a key yet.
///
/// The distinction the settings sample's exit criteria are written against:
/// "any key with no reader is labelled as such". A screen that offers a control
/// which silently does nothing is worse than one that says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStatus {
    /// A reader in this module answers it, so writing it changes a frame or a
    /// mix.
    Read,
    /// Named by `docs/plan/15-windowing.md`'s catalogue and read by nothing.
    ///
    /// Named anyway, and now rather than later, because a key named late is a
    /// file every existing player has already written — `docs/plan/14-persistence.md`'s
    /// second catalogue rule.
    Named,
}

/// One key the engine's settings catalogue defines.
///
/// **`PartialEq` and not `Eq`**, since [`Kind`] holds floats.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogueKey {
    /// The dotted key, as it is written in `settings.toml` and passed to
    /// [`SettingsStack::get`].
    pub key: String,
    /// The name the console types, which is the key without its namespace.
    ///
    /// The spelling `docs/plan/52-debug-console.md` decision 2 fixes for a
    /// settings-backed variable: `antialiasing`, not `engine.video.antialiasing`
    /// and not `r_antialiasing`, because a bare key is what the user typed in
    /// the example the plan is written against. It is `&'static str` where
    /// [`key`](Self::key) is a `String` because that is what it comes from —
    /// [`VIDEO_KEYS`] and its siblings — and because
    /// [`Binding`] needs a name that outlives the call.
    pub name: &'static str,
    /// What the key accepts, as the console's own domain type.
    ///
    /// **Was prose until `docs/plan/52-debug-console.md` decision 3.** A string
    /// could say "1 to 16" while the setter clamped to something else, and
    /// nothing could tell; a [`Kind`] is what a value is coerced and
    /// range-checked through, so
    /// `every_numeric_kind_agrees_with_the_setter_that_writes_it` can hold the
    /// two together. The prose that was here is [`help`](Self::help).
    pub kind: Kind,
    /// What the key is for, in one line — the prose the domain used to carry,
    /// minus whatever [`kind`](Self::kind) now states exactly.
    pub help: &'static str,
    /// Whether a reader answers it; see [`KeyStatus`].
    pub status: KeyStatus,
}

/// The help line every [`VIDEO_KEYS`] switch wears.
///
/// One line for six keys because it is one sentence about six keys — the name
/// is what says which effect, and a line per switch would be six copies of the
/// same clause. [`catalogue`] and the switch's own [`Binding`] read this
/// constant rather than each spelling it.
const EFFECT_HELP: &str = "whether the player allows this effect; absent allows it";

/// [`ANTIALIASING_KEY`]'s help line, for [`catalogue`] and its [`Binding`].
const ANTIALIASING_HELP: &str = "which resolve the frame gets; absent leaves the game's own tier";

/// [`RENDER_SCALE_KEY`]'s help line, for [`catalogue`] and its [`Binding`].
const RENDER_SCALE_HELP: &str = "fraction of the surface extent the frame is drawn at";

/// [`ANISOTROPIC_FILTERING_KEY`]'s help line, for [`catalogue`] and its
/// [`Binding`].
const ANISOTROPIC_FILTERING_HELP: &str = "how the base-colour page is filtered; the low end is off, and the \
     device's own ceiling clamps it";

/// [`FRAME_LIMIT_KEY`]'s help line, for [`catalogue`] and its [`Binding`].
const FRAME_LIMIT_HELP: &str = "frames a second the loop is held under; zero is unlimited";

/// The domain of every `[engine.audio]` gain: what [`audio_gains`] clamps to.
const GAIN_KIND: Kind = Kind::Float { min: 0.0, max: 1.0 };

/// The help line every `[engine.audio]` gain wears, on [`EFFECT_HELP`]'s terms.
const GAIN_HELP: &str = "the bus gain; absent is unity";

/// What a [`KeyStatus::Named`] key's [`Binding`] carries: the settings stack is
/// still its storage, and nothing may write it.
///
/// [`Flags::READ_ONLY`] is the console half of [`KeyStatus::Named`] — the plan's
/// decision 3, so `help` lists the whole catalogue instead of hiding the part of
/// it no frame reads.
const NAMED_FLAGS: Flags = Flags::ARCHIVE.union(Flags::READ_ONLY);

/// The help line of each [`NAMED_VIDEO_KEYS`] row, in that table's order.
///
/// Its own table so the console's binding for a row and the catalogue's entry
/// for it read the same literal: a `static Binding` needs a `&'static str` in a
/// const initializer, which is what stops the sentence being written twice and
/// then edited once.
///
/// **Each one opens with what the row's [`KeyStatus::Named`] means**, in the
/// words `docs/plan/52-debug-console.md` decision 3 asks the console to print,
/// because that is the fact a person reading `help` needs before the rest of the
/// line is worth anything.
const NAMED_HELP: [&str; 8] = [
    "nothing reads this yet — how the window sits on the desktop",
    "nothing reads this yet — monitor name; absent means wherever the window is",
    "nothing reads this yet — [width, height] in device pixels, as a TOML array",
    "nothing reads this yet — how the swapchain paces presentation",
    "nothing reads this yet — a scalar multiplier applied in the tonemap pass",
    "nothing reads this yet — whether the swapchain asks for an HDR format",
    "nothing reads this yet — a multiplier over the window's own scale factor",
    "nothing reads this yet — the vertical field of view in degrees",
];

/// The `[engine.video]` rows nothing reads yet, with the kinds and the help
/// `docs/plan/15-windowing.md` fixed for them.
///
/// Literals, unlike the rows below them, because there is nothing in the tree
/// to derive them from — that is exactly what makes them [`KeyStatus::Named`].
/// A row leaves this list by growing a reader and joining `catalogue`'s derived
/// half, so the two halves cannot both claim one key.
///
/// **Every range here is this list's own**, and that is the honest half of the
/// same fact: there is no setter to agree with, which is why
/// `every_numeric_kind_agrees_with_the_setter_that_writes_it` can only cover the
/// derived rows. A row that grows a reader takes the reader's range with it, and
/// joins the test at the same moment. Until then every one of them is
/// [`Flags::READ_ONLY`] to the console, so no range here decides anything.
///
/// `resolution` is [`Kind::Text`] rather than a pair, because it is a TOML array
/// and the console's domain type spells no array; `monitor` is text because a
/// monitor name is text.
const NAMED_VIDEO_KEYS: [(&str, Kind, &str); 8] = [
    (
        "display_mode",
        Kind::Enum(&["windowed", "borderless"]),
        NAMED_HELP[0],
    ),
    ("monitor", Kind::Text, NAMED_HELP[1]),
    ("resolution", Kind::Text, NAMED_HELP[2]),
    (
        "present_mode",
        Kind::Enum(&["auto", "vsync", "adaptive", "off"]),
        NAMED_HELP[3],
    ),
    (
        "brightness",
        Kind::Float { min: 0.0, max: 2.0 },
        NAMED_HELP[4],
    ),
    ("hdr_output", Kind::Bool, NAMED_HELP[5]),
    (
        "ui_scale",
        Kind::Float {
            min: 0.25,
            max: 4.0,
        },
        NAMED_HELP[6],
    ),
    (
        "fov",
        Kind::Float {
            min: 1.0,
            max: 179.0,
        },
        NAMED_HELP[7],
    ),
];

/// Every key the engine defines, read or merely named.
///
/// **Derived from the readers wherever there is a reader**, so a key cannot
/// appear here under one spelling and be read under another: the effect rows
/// come from [`VIDEO_KEYS`], the antialiasing row from [`ANTIALIASING_KEY`], the
/// scale row from [`RENDER_SCALE_KEY`], the anisotropy row from
/// [`ANISOTROPIC_FILTERING_KEY`], and the volume rows from
/// [`Bus::settings_key`]. Only the rows with no reader are
/// written out, because there is nothing to derive them from.
///
/// The **kinds** are derived too wherever a reader fixes one: the antialiasing
/// row's set is [`ANTIALIASING_NAMES`], and every numeric row's range is the one
/// its setter clamps to, which
/// `every_numeric_kind_agrees_with_the_setter_that_writes_it` holds it to.
///
/// A `Vec` rather than a `const`: a dotted key is its namespace and its name
/// joined, and `format!` is not a const operation. The caller is a settings
/// screen or `crcbl settings list`, neither of which runs per frame.
///
/// What is **not** here is the `[game]` namespace. Those keys belong to
/// whichever game wrote them, so a key this list does not name is unknown to
/// the *engine* rather than wrong.
#[must_use]
pub fn catalogue() -> Vec<CatalogueKey> {
    let read = |namespace: &str, name: &'static str, kind, help| CatalogueKey {
        key: format!("{namespace}.{name}"),
        name,
        kind,
        help,
        status: KeyStatus::Read,
    };
    let mut keys: Vec<CatalogueKey> = VIDEO_KEYS
        .iter()
        .map(|(key, _)| read(VIDEO_NAMESPACE, key, Kind::Bool, EFFECT_HELP))
        .collect();
    keys.push(read(
        VIDEO_NAMESPACE,
        ANTIALIASING_KEY,
        Kind::Enum(&ANTIALIASING_NAMES),
        ANTIALIASING_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        RENDER_SCALE_KEY,
        Kind::Float {
            min: MIN_RENDER_SCALE,
            max: 1.0,
        },
        RENDER_SCALE_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        ANISOTROPIC_FILTERING_KEY,
        Kind::Float {
            min: 1.0,
            max: MAX_ANISOTROPIC_FILTERING,
        },
        ANISOTROPIC_FILTERING_HELP,
    ));
    keys.push(read(
        VIDEO_NAMESPACE,
        FRAME_LIMIT_KEY,
        Kind::Int {
            min: 0,
            max: FRAME_LIMIT_CEILING,
        },
        FRAME_LIMIT_HELP,
    ));
    keys.extend(NAMED_VIDEO_KEYS.map(|(key, kind, help)| CatalogueKey {
        key: format!("{VIDEO_NAMESPACE}.{key}"),
        name: key,
        kind,
        help,
        status: KeyStatus::Named,
    }));
    keys.extend(
        Bus::ALL.map(|bus| read(AUDIO_NAMESPACE, bus.settings_key(), GAIN_KIND, GAIN_HELP)),
    );
    keys
}

/// What the catalogue says about `key`, or `None` for one it does not name.
///
/// The lookup a settings screen and `crcbl settings list` both make, and the
/// reason [`catalogue`] is a list rather than a map: the whole catalogue scanned
/// once per key in a player's file is not worth a hash, and the order is what a
/// screen renders in.
#[must_use]
pub fn catalogued(key: &str) -> Option<CatalogueKey> {
    catalogue().into_iter().find(|entry| entry.key == key)
}

// ── Applying a key ──────────────────────────────────────────────────────────

/// A seam the host does not have.
///
/// Not an error: a settings screen with no renderer, a headless run with no
/// mixer and the engine's own loop fixture are all hosts that legitimately
/// cannot show a key, and every one of them still wants the key written. It is
/// reported rather than swallowed because the alternative is
/// "not implemented" arriving as "applied" — the failure
/// `docs/plan/40-profiling.md` names for counters and this file's [`KeyStatus`]
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
/// owns. So the bundle keeps the pair `docs/plan/52-debug-console.md` decision 3
/// puts on it, [`GpuStage`] is the one line that forwards to it, and this is the
/// vocabulary [`apply`] speaks.
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
}

impl Deferred {
    /// Nothing recorded.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            video: None,
            gains: [None; Bus::ALL.len()],
            frame_limit: None,
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

    /// Whether anything is waiting to be applied.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.video.is_none() && self.frame_limit.is_none() && self.gains.iter().all(Option::is_none)
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
/// reason `docs/plan/52-debug-console.md` decision 3 asked for it: until this
/// existed the fan-out was `apps/options`', per key, so a console — or a second
/// screen — would have had to copy it, and a copy is where the two drift.
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
        // Every remaining `Read` key is one of `VIDEO_KEYS`, whose entry in the
        // catalogue is derived from that table — so a name that reaches here and
        // matches nothing is a key catalogued as read with no writer, which the
        // catalogue tests already refuse.
        name => {
            let Value::Bool(on) = *value else {
                unreachable!("an effect key is a bool kind, which `check` has held it to")
            };
            let (_, effect) = VIDEO_KEYS
                .into_iter()
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

// ── The renderer half of a `GameGpu` forward ────────────────────────────────

/// Put `video` into force on `renderer`, through the `device` that built it.
///
/// **The body every bundle's
/// [`GameGpu::apply_video`](crate::engine::GameGpu::apply_video) forwards to.**
/// Every bundle in `apps/` that holds a
/// [`ForwardRenderer`](crcbl_render::ForwardRenderer) has to do exactly this,
/// and a copy each is a chance each to forget the effect request or to hand the
/// scale to the anisotropy; there is one copy here and a line each there.
///
/// It writes all three of the renderer's player-facing knobs rather than the one
/// that moved, because [`VideoSettings`] is the section and a caller holding one
/// does not know which key produced it. Setting a knob to the value it already
/// holds costs nothing —
/// [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy)
/// returns early on a bit-equal ask, and the other two are field writes.
///
/// The frame ceiling in `video` is deliberately **not** applied: it is the
/// loop's, not the renderer's, and [`Stage::set_frame_limit`] is where it goes.
///
/// # Errors
///
/// [`Unsupported`] where the device refused the sampler the anisotropy asked
/// for. The renderer is left holding the sampler it had — that is
/// [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy)'s
/// own guarantee — and the failure is **also** logged, naming the key, on this
/// module's "a line that does nothing
/// says so" terms: `Unsupported` says the value did not reach the frame, and the
/// log line is where what the device said survives.
pub fn apply_video_to(
    renderer: &mut crcbl_render::ForwardRenderer,
    device: &dyn crcbl_hal::Device,
    video: &VideoSettings,
) -> Result<(), Unsupported> {
    renderer.set_render_scale(video.render_scale);
    let mut request = renderer.effect_request();
    request.video = video.effects;
    request.antialiasing = video.antialiasing;
    renderer.set_effect_request(request);
    renderer
        .set_anisotropy(device, video.anisotropic_filtering)
        .map_err(|error| {
            crcbl_core::log::warn!(
                "settings: `{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}` did not reach the \
                 frame: {error}; the page is still sampled at {}",
                renderer.anisotropy()
            );
            Unsupported
        })
}

/// Which of [`ForwardRenderer`](crcbl_render::ForwardRenderer)'s debug
/// switches `view` turns on.
///
/// In
/// [`ForwardRenderer::debug_view`](crcbl_render::ForwardRenderer::debug_view)'s
/// own precedence order — motion, occlusion, heatmap, LOD, normals — so
/// `debug_view` of a renderer this was
/// applied to answers back the view it was handed, whichever it was. That
/// round trip is what
/// `every_debug_view_sets_exactly_the_switch_its_precedence_reads_back` asserts
/// without a device.
///
/// A `match` on the whole enum rather than five comparisons, so a
/// [`DebugView`](crcbl_render::DebugView) variant added later fails to compile
/// here instead of silently drawing the shaded frame.
#[must_use]
pub const fn debug_view_switches(view: crcbl_render::DebugView) -> [bool; 5] {
    use crcbl_render::DebugView as V;
    match view {
        V::Shaded => [false, false, false, false, false],
        V::Motion => [true, false, false, false, false],
        V::AmbientOcclusion => [false, true, false, false, false],
        V::Heatmap => [false, false, true, false, false],
        V::LodTint => [false, false, false, true, false],
        V::Normals => [false, false, false, false, true],
    }
}

/// Draw `view` on `renderer` instead of the shaded picture.
///
/// **The body every bundle's
/// [`GameGpu::set_debug_view`](crate::engine::GameGpu::set_debug_view) forwards
/// to**, on [`apply_video_to`]'s terms. It writes **every** switch rather than
/// the one the view names, because the five are independent and
/// [`ForwardRenderer::debug_view`](crcbl_render::ForwardRenderer::debug_view)
/// resolves them by precedence: leaving an outer one standing would draw a view
/// nobody asked for. [`debug_view_switches`] is the table.
pub const fn set_debug_view_on(
    renderer: &mut crcbl_render::ForwardRenderer,
    view: crcbl_render::DebugView,
) {
    let [motion, occlusion, heatmap, lod, normals] = debug_view_switches(view);
    renderer.set_motion_view(motion);
    renderer.set_occlusion_view(occlusion);
    renderer.set_heatmap(heatmap);
    renderer.set_lod_view(lod);
    renderer.set_normals_view(normals);
}

// ── The console's variables ─────────────────────────────────────────────────

/// The state a settings console variable reads and writes: the player's stack,
/// and what a write still owes the process.
///
/// **The type every [`console_bindings`] binding downcasts `&mut dyn Any` to**,
/// and the reason it owns its two halves rather than borrowing them:
/// [`Any`] is implemented only for `'static` types, so a host cannot hold the
/// renderer or the mixer a write has to reach. It holds the stack, which it can
/// own, and a [`Deferred`] — see that type. `Loop::new` builds one and the
/// frame drains it; that is `docs/plan/52-debug-console.md`'s slice 5.
#[derive(Debug, Default)]
pub struct ConsoleHost {
    stack: SettingsStack,
    pending: Deferred,
    engine: crate::debug_console::EngineLink,
}

impl ConsoleHost {
    /// A host over `stack`, with nothing pending and nowhere to save.
    #[must_use]
    pub const fn new(stack: SettingsStack) -> Self {
        Self {
            stack,
            pending: Deferred::new(),
            engine: crate::debug_console::EngineLink::new(),
        }
    }

    /// This host, with `save` writing the platform settings file for
    /// `app_name`.
    ///
    /// Left unset by [`new`](Self::new) rather than defaulted to the game's
    /// name, because the run that must not write one is exactly the run that
    /// would not notice: a golden comparison or a determinism harness takes its
    /// settings from [`SettingsSource::None`](crate::engine::SettingsSource) and
    /// must not persist into whichever home directory it executes in. The
    /// caller that read a file is the caller that names it.
    #[must_use]
    pub fn saving_as(mut self, app_name: &str) -> Self {
        self.engine.app_name = Some(app_name.to_owned());
        self
    }

    /// The settings as they stand.
    #[must_use]
    pub const fn stack(&self) -> &SettingsStack {
        &self.stack
    }

    /// The settings, to write — what `save` and `dump` reach.
    pub const fn stack_mut(&mut self) -> &mut SettingsStack {
        &mut self.stack
    }

    /// What a write has asked for and nothing has applied yet.
    pub const fn pending_mut(&mut self) -> &mut Deferred {
        &mut self.pending
    }

    /// What the console's commands have asked of the loop, to read.
    #[must_use]
    pub const fn engine(&self) -> &crate::debug_console::EngineLink {
        &self.engine
    }

    /// What the console's commands have asked of the loop, to record and to
    /// drain.
    pub const fn engine_mut(&mut self) -> &mut crate::debug_console::EngineLink {
        &mut self.engine
    }
}

crcbl_console::concommand! {
    /// Write the settings file. Nothing a console sets is saved until this runs.
    pub fn save(cx, _args) {
        let saved = {
            let host = cx
                .host_mut()
                .downcast_mut::<ConsoleHost>()
                .expect("the engine's console is only ever run over a `ConsoleHost`");
            let Some(app_name) = host.engine.app_name.clone() else {
                // Deliberately a fault rather than a quiet success: a run with
                // no file to write is the golden-run case, and "saved" arriving
                // for a write that went nowhere is the failure this module's
                // `Unsupported` exists to refuse.
                return Err(Fault::new(
                    "this run reads no settings file, so there is nowhere to save to",
                ));
            };
            host.stack
                .save_platform(&app_name)
                .map_err(|error| Fault::new(error.to_string()))?;
            app_name
        };
        cx.print(format!("settings saved for `{saved}`"));
        Ok(())
    }
}

crcbl_console::concommand! {
    /// Print every key the settings stack holds, layer by layer.
    pub fn dump(cx, _args) {
        let text = cx
            .host()
            .downcast_ref::<ConsoleHost>()
            .expect("the engine's console is only ever run over a `ConsoleHost`")
            .stack
            .dump();
        for line in text.lines() {
            cx.print(line);
        }
        Ok(())
    }
}

/// Every catalogue key as a console variable, in [`catalogue`]'s order.
///
/// One [`Binding`] per key: the key's bare name as the console name
/// ([`CatalogueKey::name`]), the catalogue's [`Kind`] and help, [`Flags::ARCHIVE`]
/// because the settings stack is where the value lives, and
/// [`Flags::READ_ONLY`] beside it for a [`KeyStatus::Named`] key so the console
/// prints the whole catalogue and refuses to write the half nothing reads.
///
/// # Why a macro over a static list, and not one generic pair of functions
///
/// A [`Binding`]'s `get` and `set` are bare `fn` pointers and are **not** handed
/// the binding they belong to, so a single pair could not know which key it was
/// called for; the name has to be baked into the function. The macro bakes it,
/// one tiny pair per key, each forwarding to the one `read`/`write` body below —
/// so there is one copy of the logic and N copies of a name, which is the
/// direction that cannot drift. `the_bindings_are_the_catalogue` holds the list
/// to [`catalogue`] itself, so a key added to one and forgotten in the other is
/// a red test rather than a variable the console does not have.
#[must_use]
pub const fn console_bindings() -> &'static [&'static Binding] {
    BINDINGS
}

/// Read `namespace.name` off a [`ConsoleHost`], as `kind`'s [`Value`].
///
/// Through the readers rather than off the stack directly, so what the console
/// prints is what the engine would read — including the clamp, the default for
/// an absent key and the warning for a line that says nothing.
fn read(host: &dyn Any, namespace: &str, name: &str, kind: Kind) -> Value {
    let stack = &host
        .downcast_ref::<ConsoleHost>()
        .expect("a settings binding is only ever given a `ConsoleHost`")
        .stack;
    if namespace == AUDIO_NAMESPACE {
        let gains = audio_gains(stack);
        let (_, gain) = gains
            .into_iter()
            .find(|(bus, _)| bus.settings_key() == name)
            .expect("every audio binding names a bus");
        return Value::Float(gain);
    }
    match name {
        FRAME_LIMIT_KEY => Value::Int(i64::from(frame_limit(stack).rate())),
        // There is no word for "the player has not picked one" — see
        // `set_video`'s docs — so an absent key reads back as the rung it leaves
        // the game on, which is what `apps/options`' row shows for it too.
        ANTIALIASING_KEY => Value::Enum(
            antialiasing(stack)
                .unwrap_or_else(|| Antialiasing::from_effects(RenderEffects::DEFAULT_STACK))
                .name(),
        ),
        RENDER_SCALE_KEY => Value::Float(render_scale(stack)),
        ANISOTROPIC_FILTERING_KEY => Value::Float(anisotropic_filtering(stack)),
        _ => match VIDEO_KEYS
            .into_iter()
            .find(|(candidate, _)| *candidate == name)
        {
            Some((_, effect)) => Value::Bool(video_effects(stack).contains(effect)),
            // A `Named` key: nothing reads it, so there is nothing to read it
            // back through. Its binding is `READ_ONLY`, and this is what `help`
            // prints beside it.
            None => named_value(stack, namespace, name, kind),
        },
    }
}

/// What a [`KeyStatus::Named`] key reads back as: whatever the file holds,
/// coerced through the kind the catalogue declared for it.
///
/// Straight off the stack rather than through a reader, because the whole of
/// what makes the key `Named` is that it has no reader. A key the file does not
/// hold — the ordinary case — reads back as the kind's own floor, which is what
/// `help` prints beside "nothing reads this yet".
fn named_value(stack: &SettingsStack, namespace: &str, name: &str, kind: Kind) -> Value {
    let dotted = format!("{namespace}.{name}");
    match kind {
        Kind::Bool => Value::Bool(stack.get::<bool>(&dotted).unwrap_or_default()),
        Kind::Int { min, .. } => Value::Int(stack.get::<i64>(&dotted).unwrap_or(min)),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a settings float is read as f64 and shown as f32, which is the width every other reader here answers in"
        )]
        // Not clamped to the kind: the file holds what a hand-edit put there,
        // and a row that reported the floor instead would be this reader
        // inventing the value it was asked to show. Nothing can be set through
        // it either way — the binding is `READ_ONLY`.
        Kind::Float { min, .. } => {
            Value::Float(stack.get::<f64>(&dotted).map_or(min, |value| value as f32))
        }
        Kind::Enum(values) => Value::Enum(
            stack
                .get::<String>(&dotted)
                .and_then(|held| {
                    values
                        .iter()
                        .copied()
                        .find(|candidate| *candidate == held.as_str())
                })
                .unwrap_or(values[0]),
        ),
        Kind::Text => Value::Text(stack.get::<String>(&dotted).unwrap_or_default()),
    }
}

/// Write `namespace.name` through [`apply`], on the host's own [`Deferred`]
/// stage.
///
/// # Errors
///
/// [`apply`]'s.
fn write(host: &mut dyn Any, namespace: &str, name: &str, value: &Value) -> Result<(), Fault> {
    let host = host
        .downcast_mut::<ConsoleHost>()
        .expect("a settings binding is only ever given a `ConsoleHost`");
    apply(
        &mut host.stack,
        &format!("{namespace}.{name}"),
        value,
        &mut host.pending,
    )
    .map(|_| ())
}

/// One [`Binding`] per catalogue key, each with its own name baked in.
macro_rules! settings_bindings {
    ($(
        $binding:ident: $namespace:expr, $name:expr, $kind:expr, $flags:expr, $help:expr;
    )*) => {
        $(
            static $binding: Binding = {
                fn get(host: &dyn Any) -> Value {
                    read(host, $namespace, $name, $kind)
                }
                fn set(host: &mut dyn Any, value: &Value) -> Result<(), Fault> {
                    write(host, $namespace, $name, value)
                }
                Binding::new($name, $help, $kind, $flags, get, set)
            };
        )*

        /// The list [`console_bindings`] answers with.
        static BINDINGS: &[&Binding] = &[$(&$binding),*];
    };
}

settings_bindings! {
    SHADOWS: VIDEO_NAMESPACE, VIDEO_KEYS[0].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    AMBIENT_OCCLUSION: VIDEO_NAMESPACE, VIDEO_KEYS[1].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    REFLECTIONS: VIDEO_NAMESPACE, VIDEO_KEYS[2].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    BLOOM: VIDEO_NAMESPACE, VIDEO_KEYS[3].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    VOLUMETRIC_FOG: VIDEO_NAMESPACE, VIDEO_KEYS[4].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;
    AUTO_EXPOSURE: VIDEO_NAMESPACE, VIDEO_KEYS[5].0, Kind::Bool, Flags::ARCHIVE, EFFECT_HELP;

    ANTIALIASING: VIDEO_NAMESPACE, ANTIALIASING_KEY, Kind::Enum(&ANTIALIASING_NAMES),
        Flags::ARCHIVE, ANTIALIASING_HELP;
    RENDER_SCALE: VIDEO_NAMESPACE, RENDER_SCALE_KEY,
        Kind::Float { min: MIN_RENDER_SCALE, max: 1.0 }, Flags::ARCHIVE, RENDER_SCALE_HELP;
    ANISOTROPIC_FILTERING: VIDEO_NAMESPACE, ANISOTROPIC_FILTERING_KEY,
        Kind::Float { min: 1.0, max: MAX_ANISOTROPIC_FILTERING }, Flags::ARCHIVE,
        ANISOTROPIC_FILTERING_HELP;
    FRAME_LIMIT: VIDEO_NAMESPACE, FRAME_LIMIT_KEY,
        Kind::Int { min: 0, max: FRAME_LIMIT_CEILING }, Flags::ARCHIVE, FRAME_LIMIT_HELP;

    DISPLAY_MODE: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[0].0, NAMED_VIDEO_KEYS[0].1,
        NAMED_FLAGS, NAMED_HELP[0];
    MONITOR: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[1].0, NAMED_VIDEO_KEYS[1].1,
        NAMED_FLAGS, NAMED_HELP[1];
    RESOLUTION: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[2].0, NAMED_VIDEO_KEYS[2].1,
        NAMED_FLAGS, NAMED_HELP[2];
    PRESENT_MODE: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[3].0, NAMED_VIDEO_KEYS[3].1,
        NAMED_FLAGS, NAMED_HELP[3];
    BRIGHTNESS: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[4].0, NAMED_VIDEO_KEYS[4].1,
        NAMED_FLAGS, NAMED_HELP[4];
    HDR_OUTPUT: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[5].0, NAMED_VIDEO_KEYS[5].1,
        NAMED_FLAGS, NAMED_HELP[5];
    UI_SCALE: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[6].0, NAMED_VIDEO_KEYS[6].1,
        NAMED_FLAGS, NAMED_HELP[6];
    FOV: VIDEO_NAMESPACE, NAMED_VIDEO_KEYS[7].0, NAMED_VIDEO_KEYS[7].1,
        NAMED_FLAGS, NAMED_HELP[7];

    MASTER_VOLUME: AUDIO_NAMESPACE, Bus::ALL[0].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    MUSIC_VOLUME: AUDIO_NAMESPACE, Bus::ALL[1].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    SFX_VOLUME: AUDIO_NAMESPACE, Bus::ALL[2].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    UI_VOLUME: AUDIO_NAMESPACE, Bus::ALL[3].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    VOICE_VOLUME: AUDIO_NAMESPACE, Bus::ALL[4].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
    AMBIENCE_VOLUME: AUDIO_NAMESPACE, Bus::ALL[5].settings_key(), GAIN_KIND,
        Flags::ARCHIVE, GAIN_HELP;
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_store::MemoryStorage;
    use crcbl_store::StorageSource;
    use crcbl_store::settings::SETTINGS_FILE;

    /// A stack over `storage`, edited by `edit`, saved back, and reloaded
    /// through the real loader.
    ///
    /// **The round trip is the point.** A writer that serialises into an
    /// in-memory table proves nothing about the file: the section header, the
    /// TOML type each key lands as, and whether `save` ever ran are all between
    /// the two halves, and every one of them is a way for a settings screen to
    /// keep a value the next start-up will not read.
    fn round_trip(edit: impl FnOnce(&mut SettingsStack)) -> (SettingsStack, String) {
        let storage = MemoryStorage::new();
        let path = std::path::Path::new(SETTINGS_FILE);
        let mut stack = SettingsStack::from_storage(&storage);
        edit(&mut stack);
        stack.save(&storage, path).expect("memory storage saves");
        let written = String::from_utf8(storage.read(path).expect("the save wrote a file"))
            .expect("the writer emits UTF-8");
        (SettingsStack::from_storage(&storage), written)
    }

    /// **What a settings screen writes is what the next start-up reads.**
    ///
    /// Every field of [`VideoSettings`] at once, and none of them at its
    /// default: a round trip that carried nothing would still pass if the
    /// values under test were the ones an empty file already answers.
    #[test]
    fn a_saved_video_section_reads_back_unchanged() {
        let wanted = VideoSettings {
            effects: RenderEffects::all() - RenderEffects::BLOOM - RenderEffects::SHADOWS,
            antialiasing: Some(Antialiasing::Smaa),
            render_scale: 0.5,
            anisotropic_filtering: 4.0,
            frame_limit: FrameLimit::fps(60),
        };
        let (reloaded, _) = round_trip(|stack| {
            set_video(stack, wanted).expect("a fresh user layer accepts every key");
        });
        assert_eq!(video(&reloaded), wanted);
    }

    /// **The anisotropy is clamped on the way in as well as on the way out**,
    /// to this layer's range and not the device's — the file is the ask, and
    /// the setter meets the device.
    #[test]
    fn an_anisotropy_past_the_desktop_ceiling_is_stored_at_the_ceiling() {
        let (reloaded, written) = round_trip(|stack| {
            set_anisotropic_filtering(stack, 64.0).expect("a fresh user layer accepts every key");
        });
        assert!(
            (anisotropic_filtering(&reloaded) - MAX_ANISOTROPIC_FILTERING).abs() < f32::EPSILON,
            "reads back {}",
            anisotropic_filtering(&reloaded)
        );
        assert!(
            !written.contains("64"),
            "the file kept the unclamped ask:\n{written}"
        );
    }

    /// **An effect left on is written as `true`, not left out.**
    ///
    /// The reader cannot tell those apart and a writer must: a screen that only
    /// ever wrote `false` could turn an effect off and never back on, since the
    /// key it would have to remove is the one it never wrote.
    #[test]
    fn an_effect_left_standing_is_still_a_row_in_the_file() {
        let (_, written) = round_trip(|stack| {
            set_video_effects(stack, RenderEffects::all() - RenderEffects::BLOOM)
                .expect("a fresh user layer accepts every key");
        });
        assert!(
            written.contains("shadows = true"),
            "an effect the player kept is missing from the file:\n{written}"
        );
        assert!(
            written.contains("bloom = false"),
            "an effect the player switched off is missing from the file:\n{written}"
        );
    }

    /// **The clamp is on the way in as well as on the way out**, so the file
    /// holds the scale that will be drawn rather than the one that was asked
    /// for.
    #[test]
    fn a_scale_below_the_floor_is_stored_at_the_floor() {
        let (reloaded, written) = round_trip(|stack| {
            set_render_scale(stack, 0.05).expect("a fresh user layer accepts every key");
        });
        assert!(
            (render_scale(&reloaded) - MIN_RENDER_SCALE).abs() < f32::EPSILON,
            "reads back {}",
            render_scale(&reloaded)
        );
        assert!(
            !written.contains("0.05"),
            "the file kept the unclamped ask:\n{written}"
        );
    }

    /// **A gain reaches its own bus and no other**, under
    /// [`Bus::settings_key`]'s spelling on both sides.
    #[test]
    fn a_saved_gain_reads_back_on_its_own_bus() {
        let (reloaded, _) = round_trip(|stack| {
            set_audio_gain(stack, Bus::Sfx, 0.25).expect("a fresh user layer accepts every key");
        });
        for (bus, gain) in audio_gains(&reloaded) {
            let wanted = if bus == Bus::Sfx { 0.25 } else { 1.0 };
            assert!(
                (gain - wanted).abs() < f32::EPSILON,
                "{bus:?} reads {gain}, wanted {wanted}"
            );
        }
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

    /// **Every key a reader answers is in the catalogue, under the spelling
    /// the reader uses.**
    ///
    /// The failure this exists for is a catalogue that drifts: a screen offers
    /// `engine.video.ssao`, the reader looks for `ambient_occlusion`, and the
    /// player's choice lands in a key nothing will ever read. Asserted against
    /// the reader tables themselves rather than a second list, so a key renamed
    /// in one place fails here rather than in a player's file.
    #[test]
    fn every_key_with_a_reader_is_catalogued_as_read() {
        let read: Vec<String> = catalogue()
            .into_iter()
            .filter(|entry| entry.status == KeyStatus::Read)
            .map(|entry| entry.key)
            .collect();

        let mut wanted: Vec<String> = VIDEO_KEYS
            .iter()
            .map(|(key, _)| format!("{VIDEO_NAMESPACE}.{key}"))
            .collect();
        wanted.push(format!("{VIDEO_NAMESPACE}.{ANTIALIASING_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}"));
        wanted.push(format!("{VIDEO_NAMESPACE}.{FRAME_LIMIT_KEY}"));
        wanted.extend(Bus::ALL.map(|bus| format!("{AUDIO_NAMESPACE}.{}", bus.settings_key())));

        for key in &wanted {
            assert!(read.contains(key), "`{key}` has a reader and no entry");
        }
        assert_eq!(
            read.len(),
            wanted.len(),
            "the catalogue calls something read that no reader answers: {read:?}"
        );
    }

    /// **A key is named once**, so a lookup cannot be ambiguous and a screen
    /// cannot draw one control twice.
    ///
    /// The way this breaks is a row keeping its `Named` entry after it grows a
    /// reader, which would put the same key in the list under both statuses.
    #[test]
    fn no_key_appears_in_the_catalogue_twice() {
        let mut seen: Vec<String> = catalogue().into_iter().map(|entry| entry.key).collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(before, seen.len(), "a key is catalogued twice: {seen:?}");
    }

    /// **A key the catalogue names is a key a stack can be asked for**, which
    /// is the claim a screen depends on and the one a stray space or a wrong
    /// namespace would break silently.
    #[test]
    fn every_catalogued_key_is_a_usable_dotted_key() {
        let stack = stack_from("");
        for entry in catalogue() {
            assert!(
                entry.key.starts_with("engine.")
                    && !entry.key.contains(' ')
                    && entry.key.split('.').count() == 3,
                "`{}` is not a two-level engine key",
                entry.key
            );
            assert!(
                !stack.contains(&entry.key),
                "an empty file answered `{}`",
                entry.key
            );
            assert!(!entry.help.is_empty(), "`{}` has no help", entry.key);
            assert!(
                entry.key.ends_with(entry.name) && !entry.name.contains('.'),
                "`{}` does not end in its bare console name `{}`",
                entry.key,
                entry.name
            );
        }
    }

    /// **A key nothing defines is not catalogued**, which is what lets a caller
    /// tell a typo from a `[game]` key.
    #[test]
    fn a_key_the_engine_does_not_define_is_not_catalogued() {
        assert!(
            catalogued("engine.video.shadow").is_none(),
            "a typo matched"
        );
        assert!(
            catalogued("game.difficulty").is_none(),
            "a game key matched"
        );
        assert_eq!(
            catalogued(&format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}"))
                .expect("the scale is catalogued")
                .status,
            KeyStatus::Read,
        );
        assert_eq!(
            catalogued(&format!("{VIDEO_NAMESPACE}.display_mode"))
                .expect("the display mode is catalogued")
                .status,
            KeyStatus::Named,
            "a key with no reader must not claim to have one",
        );
    }

    /// A stack over a settings file with `toml` in it, through the real
    /// loader — not a table built in memory, because the spelling of the
    /// section header is half of what this module has to get right.
    fn stack_from(toml: &str) -> SettingsStack {
        let storage = MemoryStorage::new();
        storage
            .write(std::path::Path::new(SETTINGS_FILE), toml.as_bytes())
            .expect("memory storage accepts every write");
        SettingsStack::from_storage(&storage)
    }

    /// The gain the reader answers for `bus`, off a file holding `toml`.
    fn gain_of(toml: &str, bus: Bus) -> f32 {
        audio_gains(&stack_from(toml))
            .into_iter()
            .find(|(named, _)| *named == bus)
            .expect("every bus is answered for")
            .1
    }

    /// **A bus the file says nothing about is at unity.**
    ///
    /// The audio half of `an_absent_key_clamps_nothing`, and the reason it is
    /// worth its own test: an absent video key leaves an effect *on* because
    /// that layer may only clamp downward, while an absent audio key is unity
    /// because the key **is** the gain. The two arrive at "nothing changes" for
    /// different reasons, so an implementation that confused them would still
    /// pass one of these.
    #[test]
    fn a_file_with_no_audio_section_leaves_every_bus_at_unity() {
        for (bus, gain) in audio_gains(&stack_from("")) {
            assert!(
                (gain - 1.0).abs() < f32::EPSILON,
                "{bus:?} reads {gain} off a file that never mentions it"
            );
        }
    }

    /// **A key moves its own bus and no other**, and it is read under the
    /// spelling [`Bus::settings_key`] gives it.
    #[test]
    fn a_volume_key_moves_its_own_bus() {
        let toml = "[engine.audio]\nmusic_volume = 0.25\n";
        assert!((gain_of(toml, Bus::Music) - 0.25).abs() < f32::EPSILON);
        for bus in Bus::ALL {
            if bus == Bus::Music {
                continue;
            }
            let gain = gain_of(toml, bus);
            assert!(
                (gain - 1.0).abs() < f32::EPSILON,
                "setting the music volume moved {bus:?} to {gain}"
            );
        }
    }

    /// Every bus is reachable from a file, under a key nothing else claims.
    ///
    /// The guard against a bus added to [`Bus::ALL`] whose key is a copy of a
    /// neighbour's: the symptom is one bus unreachable and another moving two
    /// gains at once, and a per-bus test would not see it.
    #[test]
    fn every_bus_is_reachable_from_a_file_under_its_own_key() {
        for bus in Bus::ALL {
            let toml = format!("[engine.audio]\n{} = 0.5\n", bus.settings_key());
            for (named, gain) in audio_gains(&stack_from(&toml)) {
                let expected = if named == bus { 0.5 } else { 1.0 };
                assert!(
                    (gain - expected).abs() < f32::EPSILON,
                    "with only {}'s key set, {named:?} reads {gain} rather than {expected}",
                    bus.settings_key()
                );
            }
        }
    }

    /// **A whole number is a gain**, which is what a hand-edited file holds.
    ///
    /// TOML makes `= 1` an integer and `= 1.0` a float, and a reader that could
    /// not take the first would leave that key doing nothing — the failure the
    /// warn branch exists to make visible, arriving on the spelling a person is
    /// most likely to write.
    #[test]
    fn a_volume_written_as_a_whole_number_is_read() {
        assert!(gain_of("[engine.audio]\nmaster_volume = 0\n", Bus::Master).abs() < f32::EPSILON);
        let one = gain_of("[engine.audio]\nmaster_volume = 1\n", Bus::Master);
        assert!((one - 1.0).abs() < f32::EPSILON, "read {one}");
    }

    /// A gain outside `[0, 1]` is clamped rather than refused.
    ///
    /// Both ends, because they fail differently: a negative gain inverts every
    /// sample on the bus, and one above unity is the clipping the mixer has no
    /// limiter for.
    #[test]
    fn a_volume_outside_the_range_is_clamped_to_it() {
        assert!(gain_of("[engine.audio]\nsfx_volume = -2.0\n", Bus::Sfx).abs() < f32::EPSILON);
        let loud = gain_of("[engine.audio]\nsfx_volume = 40.0\n", Bus::Sfx);
        assert!((loud - 1.0).abs() < f32::EPSILON, "read {loud}");
    }

    /// A key holding something that is not a number leaves its bus at unity.
    ///
    /// `a_key_holding_something_that_is_not_a_boolean_clamps_nothing_and_warns`
    /// is the same claim on the video side; the direction here is the safe one
    /// for the same reason, since silence is what a broken line must not cause.
    #[test]
    fn a_volume_that_is_not_a_number_leaves_its_bus_alone() {
        let gain = gain_of("[engine.audio]\nui_volume = \"half\"\n", Bus::Ui);
        assert!((gain - 1.0).abs() < f32::EPSILON, "read {gain}");
    }

    /// **Every effect is reachable from a settings file**, through the boolean
    /// table or through the antialiasing ladder, and no two keys claim one bit.
    ///
    /// The guard against a bit added to [`RenderEffects`] and not to
    /// [`VIDEO_KEYS`]: the omission has no symptom of its own — the effect
    /// simply cannot be turned off, and a player's row does nothing — so
    /// nothing else would report it. The two resolve bits are the exception the
    /// ladder exists for, so they are named here as the ladder's and asserted to
    /// be **out** of the boolean table: a `smaa = false` row a player could
    /// still write is a row nothing reads.
    #[test]
    fn every_effect_has_a_key_and_no_two_share_one() {
        let mut covered = RenderEffects::empty();
        for (key, effect) in VIDEO_KEYS {
            assert!(
                !covered.intersects(effect),
                "{key} names an effect another key already names"
            );
            covered.insert(effect);
        }

        let slot = Antialiasing::ALL
            .into_iter()
            .fold(RenderEffects::empty(), |bits, tier| bits.union(tier.bits()));
        assert!(
            !covered.intersects(slot),
            "the resolve slot is a boolean row as well as a ladder rung"
        );
        assert_eq!(
            covered.union(slot),
            RenderEffects::all(),
            "an effect with no [engine.video] key is one a player cannot reach"
        );
    }

    /// **A key set to `false` removes exactly its own effect.**
    ///
    /// The pairs are written out rather than taken from [`VIDEO_KEYS`], because
    /// a table used as its own oracle cannot fail: swap two of its rows and a
    /// loop over it still agrees with itself, while every player's settings
    /// file now switches off the wrong effect. These spellings are also the
    /// compatibility promise — renaming one is a file every existing player has
    /// already written.
    #[test]
    fn a_key_set_to_false_removes_that_effect_and_no_other() {
        for (key, effect) in [
            ("shadows", RenderEffects::SHADOWS),
            ("ambient_occlusion", RenderEffects::AMBIENT_OCCLUSION),
            ("reflections", RenderEffects::REFLECTIONS),
            ("bloom", RenderEffects::BLOOM),
            ("volumetric_fog", RenderEffects::VOLUMETRIC_FOG),
            ("auto_exposure", RenderEffects::AUTO_EXPOSURE),
        ] {
            let stack = stack_from(&format!("[{VIDEO_NAMESPACE}]\n{key} = false\n"));
            assert_eq!(
                video_effects(&stack),
                RenderEffects::all().difference(effect),
                "{key} = false"
            );
        }
    }

    /// **A key that is absent is not a key that says "off".**
    ///
    /// The arm that fails if a missing key is read as `false`: an empty file,
    /// an `[engine.video]` section naming only one effect, and a key set to
    /// `true` all have to leave every unmentioned effect standing — otherwise
    /// installing the engine turns every effect off for every player who has
    /// never opened a settings screen.
    #[test]
    fn an_absent_key_clamps_nothing() {
        assert_eq!(
            video_effects(&SettingsStack::new()),
            RenderEffects::all(),
            "a stack with no layer at all"
        );
        assert_eq!(
            video_effects(&stack_from("")),
            RenderEffects::all(),
            "a settings file with nothing in it"
        );
        assert_eq!(
            video_effects(&stack_from("[game]\ndifficulty = \"normal\"\n")),
            RenderEffects::all(),
            "a settings file that never mentions video"
        );
        assert_eq!(
            video_effects(&stack_from(&format!("[{VIDEO_NAMESPACE}]\nvsync = true\n"))),
            RenderEffects::all(),
            "an [engine.video] section that names no effect"
        );
        assert_eq!(
            video_effects(&stack_from(&format!(
                "[{VIDEO_NAMESPACE}]\nshadows = false\n"
            ))),
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "one effect off must leave the two it did not name alone"
        );
    }

    /// **`true` is not "force on", because this layer cannot add.**
    ///
    /// It reads identically to an absent key, which is what makes the layer a
    /// clamp: the row a player set to on is the one the camera stack still
    /// decides.
    #[test]
    fn a_key_set_to_true_reads_the_same_as_no_key_at_all() {
        for (key, _) in VIDEO_KEYS {
            assert_eq!(
                video_effects(&stack_from(&format!("[{VIDEO_NAMESPACE}]\n{key} = true\n"))),
                RenderEffects::all(),
                "{key} = true"
            );
        }
    }

    /// **A value of the wrong type clamps nothing and warns, naming the key.**
    ///
    /// A hand-edited file cannot switch an effect off by accident and cannot
    /// fail the start-up either — and the player who wrote the line hears
    /// about it, because a setting that silently does nothing is
    /// indistinguishable from an engine that ignores the file.
    #[test]
    fn a_key_holding_something_that_is_not_a_boolean_clamps_nothing_and_warns() {
        let capture = crcbl_core::log::capture();
        let stack = stack_from(&format!("[{VIDEO_NAMESPACE}]\nshadows = \"off\"\n"));
        assert_eq!(video_effects(&stack), RenderEffects::all());

        let warned: Vec<_> = capture
            .records()
            .into_iter()
            .filter(|record| record.message.contains("engine.video.shadows"))
            .collect();
        assert_eq!(
            warned.len(),
            1,
            "exactly the key that could not be read: {:?}",
            capture.records()
        );
        assert_eq!(warned[0].level, crcbl_core::log::Level::Warn);
    }

    /// **The keys that read cleanly are silent**, so the warning above stays
    /// worth reading.
    ///
    /// Every arm of the ordinary path: absent, `true`, `false`, and a section
    /// holding a key this layer does not own. A warning that fires for a file
    /// with nothing wrong with it is one a player learns to ignore, and then
    /// the one that matters is ignored too.
    #[test]
    fn a_settings_file_this_layer_can_read_warns_about_nothing() {
        let capture = crcbl_core::log::capture();
        for toml in [
            String::new(),
            format!("[{VIDEO_NAMESPACE}]\nshadows = false\n"),
            format!("[{VIDEO_NAMESPACE}]\nreflections = true\n"),
            format!("[{VIDEO_NAMESPACE}]\nvsync = \"sometimes\"\n"),
        ] {
            let _ = video_effects(&stack_from(&toml));
        }
        let records = capture.records();
        assert!(
            records
                .iter()
                .all(|record| record.level != crcbl_core::log::Level::Warn),
            "nothing here is a mistake this layer can see: {records:?}"
        );
    }

    /// **The highest layer wins, in both directions.**
    ///
    /// `[engine.video]` is a namespace a game may ship defaults for, so the
    /// stack under the user's file is not always empty — and a read that took
    /// the first hit from the bottom, or that unioned the layers, would answer
    /// with a default the player has already overridden. Both directions,
    /// because only one of them fails for either mistake.
    ///
    /// The lower layer is a second file rather than a
    /// [`SettingsLayer::GameDefaults`](crcbl_store::settings::SettingsLayer)
    /// table: naming one would mean this crate depending on the TOML parser it
    /// deliberately reaches only through `crcbl-store`. What is being asserted
    /// is the stack's priority order, which is the same for every layer kind.
    #[test]
    fn the_highest_layer_wins_over_a_default_underneath_it() {
        let mut stack = SettingsStack::new();
        for toml in [
            // The game's defaults: shadows on, reflections off.
            format!("[{VIDEO_NAMESPACE}]\nshadows = true\nreflections = false\n"),
            // The player, disagreeing with both.
            format!("[{VIDEO_NAMESPACE}]\nshadows = false\nreflections = true\n"),
        ] {
            let storage = MemoryStorage::new();
            storage
                .write(std::path::Path::new(SETTINGS_FILE), toml.as_bytes())
                .expect("memory storage accepts every write");
            stack.add(crcbl_store::settings::SettingsLayer::UserFile(
                crcbl_store::settings::StorageSettingsFile::load(
                    &storage,
                    std::path::Path::new(SETTINGS_FILE),
                )
                .expect("a file this test wrote"),
            ));
        }

        assert_eq!(
            video_effects(&stack),
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "the player's file must beat the layer under it for both keys"
        );
    }

    /// The scale the reader answers off a file holding `toml`.
    fn scale_of(toml: &str) -> f32 {
        render_scale(&stack_from(toml))
    }

    /// **A file that says nothing draws the whole extent.**
    ///
    /// The scalar half of `an_absent_key_clamps_nothing`: a scale is not a bit,
    /// but the rule this layer lives under is the same one, and `1.0` is what
    /// "clamped nothing" spells for a size.
    #[test]
    fn an_absent_render_scale_draws_the_whole_extent() {
        assert!((scale_of("") - 1.0).abs() < f32::EPSILON);
        assert_eq!(video(&stack_from("")), VideoSettings::unrestricted());
    }

    /// **A scale is read, and clamped to the range the renderer enforces.**
    ///
    /// Both bounds, because the two are wrong in opposite directions and a
    /// clamp written with one bound passes a test that only checks the other.
    /// Above `1.0` matters most: `ForwardRenderer` would allocate a target
    /// larger than the surface for a player who typed an extra digit.
    #[test]
    fn a_render_scale_is_clamped_to_the_range_the_renderer_enforces() {
        let key = format!("[{VIDEO_NAMESPACE}]\n{RENDER_SCALE_KEY} = ");
        assert!((scale_of(&format!("{key}0.5\n")) - 0.5).abs() < f32::EPSILON);
        assert!((scale_of(&format!("{key}2.0\n")) - 1.0).abs() < f32::EPSILON);
        assert!((scale_of(&format!("{key}0.01\n")) - MIN_RENDER_SCALE).abs() < f32::EPSILON);
        assert!((scale_of(&format!("{key}-3.0\n")) - MIN_RENDER_SCALE).abs() < f32::EPSILON);
    }

    /// **A value no scale can be read out of draws the whole extent and warns,
    /// naming the key.**
    ///
    /// `nan` and `inf` are here beside the string because TOML spells them and
    /// `f32::clamp` answers NaN for NaN rather than a bound — so the arm that
    /// catches the typo is not the arm that catches these, and a reader written
    /// with only the type check would hand `begin_frame` an extent of zero
    /// pixels.
    #[test]
    fn a_render_scale_that_is_not_a_usable_number_warns_and_draws_it_all() {
        for value in ["\"half\"", "nan", "inf", "-inf"] {
            let capture = crcbl_core::log::capture();
            let toml = format!("[{VIDEO_NAMESPACE}]\n{RENDER_SCALE_KEY} = {value}\n");
            let scale = scale_of(&toml);
            assert!(
                (scale - 1.0).abs() < f32::EPSILON,
                "`{value}` was read as a scale of {scale}"
            );

            let warned: Vec<_> = capture
                .records()
                .into_iter()
                .filter(|record| {
                    record
                        .message
                        .contains(&format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}"))
                })
                .collect();
            assert_eq!(
                warned.len(),
                1,
                "`{value}` should warn exactly once: {:?}",
                capture.records()
            );
            assert_eq!(warned[0].level, crcbl_core::log::Level::Warn);
        }
    }

    /// **A scale written without a decimal point is still a scale.**
    ///
    /// TOML tells an integer from a float, and `render_scale = 1` is what a
    /// player writes for "all of it". Reading it as absent would be harmless
    /// here and is not the point: the same reader would drop `0` too, which is
    /// the value the clamp exists for.
    #[test]
    fn a_whole_number_is_read_as_a_scale() {
        let key = format!("[{VIDEO_NAMESPACE}]\n{RENDER_SCALE_KEY} = ");
        assert!((scale_of(&format!("{key}1\n")) - 1.0).abs() < f32::EPSILON);
        assert!((scale_of(&format!("{key}0\n")) - MIN_RENDER_SCALE).abs() < f32::EPSILON);
    }

    /// The anisotropy the reader answers off a file holding `toml`.
    fn anisotropy_of(toml: &str) -> f32 {
        anisotropic_filtering(&stack_from(toml))
    }

    /// **A file that says nothing samples at the engine's default**, not at
    /// the ceiling: "nothing asked" is the engine's own figure here as it is
    /// for every key, even though this one may ask above it.
    #[test]
    fn an_absent_anisotropy_is_the_engines_default() {
        assert!((anisotropy_of("") - DEFAULT_ANISOTROPY).abs() < f32::EPSILON);
    }

    /// **An anisotropy is read, and clamped to this layer's range.**
    ///
    /// Both bounds and the whole-number spelling, which is the one a player
    /// writes: `anisotropic_filtering = 16`. Above the ceiling is the ceiling
    /// and not a refusal; below one is one, the value that turns the filter
    /// off.
    #[test]
    fn an_anisotropy_is_clamped_to_the_range_the_file_may_spell() {
        let key = format!("[{VIDEO_NAMESPACE}]\n{ANISOTROPIC_FILTERING_KEY} = ");
        assert!((anisotropy_of(&format!("{key}4\n")) - 4.0).abs() < f32::EPSILON);
        assert!((anisotropy_of(&format!("{key}2.0\n")) - 2.0).abs() < f32::EPSILON);
        assert!(
            (anisotropy_of(&format!("{key}64\n")) - MAX_ANISOTROPIC_FILTERING).abs() < f32::EPSILON
        );
        assert!((anisotropy_of(&format!("{key}0\n")) - 1.0).abs() < f32::EPSILON);
        assert!((anisotropy_of(&format!("{key}-8\n")) - 1.0).abs() < f32::EPSILON);
    }

    /// **A value no anisotropy can be read out of is the default and warns,
    /// naming the key** — `render_scale`'s test, for the same arms.
    #[test]
    fn an_anisotropy_that_is_not_a_usable_number_warns_and_is_the_default() {
        for value in ["\"lots\"", "nan", "inf", "-inf"] {
            let capture = crcbl_core::log::capture();
            let toml = format!("[{VIDEO_NAMESPACE}]\n{ANISOTROPIC_FILTERING_KEY} = {value}\n");
            let anisotropy = anisotropy_of(&toml);
            assert!(
                (anisotropy - DEFAULT_ANISOTROPY).abs() < f32::EPSILON,
                "`{value}` was read as an anisotropy of {anisotropy}"
            );

            let warned: Vec<_> = capture
                .records()
                .into_iter()
                .filter(|record| {
                    record
                        .message
                        .contains(&format!("{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}"))
                })
                .collect();
            assert_eq!(
                warned.len(),
                1,
                "`{value}` should warn exactly once: {:?}",
                capture.records()
            );
            assert_eq!(warned[0].level, crcbl_core::log::Level::Warn);
        }
    }

    /// **The two halves of `[engine.video]` are read from one file and neither
    /// disturbs the other.**
    ///
    /// [`video`] is the only reader a caller uses, so a scale key that made
    /// `video_effects` warn, or an effect key that cost the scale, would reach
    /// every sample at once.
    #[test]
    fn the_scale_and_the_effect_bits_are_read_side_by_side() {
        let capture = crcbl_core::log::capture();
        let settings = video(&stack_from(&format!(
            "[{VIDEO_NAMESPACE}]\nshadows = false\n{RENDER_SCALE_KEY} = 0.75\n"
        )));
        assert_eq!(
            settings.effects,
            RenderEffects::all().difference(RenderEffects::SHADOWS)
        );
        assert!((settings.render_scale - 0.75).abs() < f32::EPSILON);

        let records = capture.records();
        assert!(
            records
                .iter()
                .all(|record| record.level != crcbl_core::log::Level::Warn),
            "a file this layer can read whole warns about nothing: {records:?}"
        );
    }

    /// The tier the reader answers off a file holding `toml`.
    fn tier_of(toml: &str) -> Option<Antialiasing> {
        antialiasing(&stack_from(toml))
    }

    /// **A file that says nothing picks no tier**, which leaves the resolve
    /// slot to the view's own stack.
    ///
    /// The distinction the [`Option`] exists for: `None` here is not
    /// [`Antialiasing::None`], which is a tier the player *chose* and which
    /// empties the slot.
    #[test]
    fn an_absent_antialiasing_key_leaves_the_games_own_tier() {
        assert_eq!(tier_of(""), None);
        assert_eq!(tier_of("[game]\ndifficulty = \"normal\"\n"), None);
        assert_eq!(
            tier_of(&format!("[{VIDEO_NAMESPACE}]\nshadows = false\n")),
            None,
            "another key must not pick a tier"
        );
        assert_eq!(video(&stack_from("")).antialiasing, None);
    }

    /// **Every rung round trips through a file**, under the one spelling
    /// [`Antialiasing::name`] gives it.
    ///
    /// Both directions on every rung, because the failure this guards is a
    /// screen that saves `"smaa"` and a start-up that reads back `"none"` — the
    /// player's whole setting, silently, with no line to tell them why.
    #[test]
    fn every_antialiasing_rung_round_trips_through_a_file() {
        for tier in Antialiasing::ALL {
            let (reloaded, written) = round_trip(|stack| {
                set_antialiasing(stack, tier).expect("a fresh user layer takes the key");
            });
            assert_eq!(antialiasing(&reloaded), Some(tier));
            assert!(
                written.contains(&format!("{ANTIALIASING_KEY} = \"{}\"", tier.name())),
                "{tier:?} left no row behind:\n{written}"
            );

            // And the hand-written spelling, which is what a player edits in.
            assert_eq!(
                tier_of(&format!(
                    "[{VIDEO_NAMESPACE}]\n{ANTIALIASING_KEY} = \"{}\"\n",
                    tier.name()
                )),
                Some(tier),
            );
        }
    }

    /// **A file still holding the boolean this key used to be reads as the
    /// meaning it had**, and says nothing about it.
    ///
    /// `true` was "the player has not asked for less", which is a tier unpicked;
    /// `false` was "no resolve at all", which is [`Antialiasing::None`]. Neither
    /// is a mistake the player made, so neither warns — a line that still means
    /// what it meant is not a line to complain about.
    #[test]
    fn a_stale_antialiasing_boolean_reads_as_the_meaning_it_had() {
        let capture = crcbl_core::log::capture();
        assert_eq!(
            tier_of(&format!("[{VIDEO_NAMESPACE}]\n{ANTIALIASING_KEY} = true\n")),
            None,
            "the old `true` was the player asking for nothing",
        );
        assert_eq!(
            tier_of(&format!(
                "[{VIDEO_NAMESPACE}]\n{ANTIALIASING_KEY} = false\n"
            )),
            Some(Antialiasing::None),
            "the old `false` was the player emptying the slot",
        );
        let records = capture.records();
        assert!(
            records
                .iter()
                .all(|record| record.level != crcbl_core::log::Level::Warn),
            "a line that still means what it meant warned: {records:?}"
        );
    }

    /// **A value that names no rung picks nothing and warns, naming the key.**
    ///
    /// The spellings are the ones a hand-edited file plausibly holds: a rung
    /// that is not built yet, the same word in the wrong case, and the numbers
    /// TOML would take for the boolean this key used to be.
    #[test]
    fn an_antialiasing_key_naming_no_rung_warns_and_picks_nothing() {
        for value in ["\"cmaa2\"", "\"FXAA\"", "\"\"", "1", "0.5"] {
            let capture = crcbl_core::log::capture();
            let toml = format!("[{VIDEO_NAMESPACE}]\n{ANTIALIASING_KEY} = {value}\n");
            assert_eq!(tier_of(&toml), None, "`{value}` was read as a tier");

            let warned: Vec<_> = capture
                .records()
                .into_iter()
                .filter(|record| {
                    record
                        .message
                        .contains(&format!("{VIDEO_NAMESPACE}.{ANTIALIASING_KEY}"))
                })
                .collect();
            assert_eq!(
                warned.len(),
                1,
                "`{value}` should warn exactly once: {:?}",
                capture.records()
            );
            assert_eq!(warned[0].level, crcbl_core::log::Level::Warn);
        }
    }

    /// **The catalogue names the tier key once, with every rung the reader
    /// takes, and names no `smaa` key at all.**
    ///
    /// The kind is [`ANTIALIASING_NAMES`], which is built from
    /// [`Antialiasing::ALL`] — so this asserts the derivation actually arrived
    /// rather than re-deriving it: a rung added to the enum and a set that did
    /// not follow is a screen offering a control it will not describe. The
    /// `smaa` half is the retired key — a catalogue that still named it would
    /// have `crcbl settings list` reporting a row nothing reads as one the
    /// engine defines.
    #[test]
    fn the_antialiasing_domain_names_every_rung_the_reader_takes() {
        let key = format!("{VIDEO_NAMESPACE}.{ANTIALIASING_KEY}");
        let entry = catalogued(&key).expect("the tier is catalogued");
        assert_eq!(entry.status, KeyStatus::Read);
        let Kind::Enum(values) = entry.kind else {
            panic!("the tier is an enum, not {:?}", entry.kind)
        };
        assert_eq!(values.len(), Antialiasing::ALL.len());
        for tier in Antialiasing::ALL {
            assert!(
                values.contains(&tier.name()),
                "{tier:?} is missing from the set {values:?}",
            );
            assert_eq!(entry.kind.parse(tier.name()), Ok(Value::Enum(tier.name())));
        }
        assert_eq!(
            catalogue().iter().filter(|row| row.key == key).count(),
            1,
            "the tier is catalogued twice",
        );
        assert!(
            catalogued(&format!("{VIDEO_NAMESPACE}.smaa")).is_none(),
            "the retired key is still catalogued",
        );
    }

    /// The ceiling `toml` puts on the frame rate.
    fn ceiling_of(toml: &str) -> FrameLimit {
        frame_limit(&stack_from(toml))
    }

    /// **A file that says nothing caps nothing**, and so does a file that says
    /// zero.
    ///
    /// The two are one test because they must give one answer: zero is
    /// [`FrameLimit::unlimited`]'s own spelling, so a player who wrote
    /// `frame_limit = 0` has asked for exactly what a player who wrote nothing
    /// asked for, and a reader that treated the row as "cap at zero fps" would
    /// stop the loop dead.
    #[test]
    fn an_absent_frame_limit_and_a_zero_one_both_cap_nothing() {
        let asked = FrameLimit::fps(144);
        for toml in ["", &format!("[{VIDEO_NAMESPACE}]\n{FRAME_LIMIT_KEY} = 0\n")] {
            let ceiling = ceiling_of(toml);
            assert_eq!(ceiling, FrameLimit::unlimited(), "read from {toml:?}");
            assert_eq!(asked.clamped_to(ceiling), asked, "read from {toml:?}");
        }
    }

    /// **The ceiling only ever takes rate away.**
    ///
    /// All three directions, because a reader that returned the file's value
    /// outright passes the middle case and fails the other two — and the case
    /// it fails is the one that matters, a game capped at 30 jumping to 60
    /// because the player asked for "at most 60".
    #[test]
    fn a_frame_limit_caps_a_game_and_never_raises_one() {
        let ceiling = ceiling_of(&format!("[{VIDEO_NAMESPACE}]\n{FRAME_LIMIT_KEY} = 60\n"));
        assert_eq!(ceiling, FrameLimit::fps(60));
        assert_eq!(
            FrameLimit::fps(144).clamped_to(ceiling),
            FrameLimit::fps(60)
        );
        assert_eq!(FrameLimit::fps(30).clamped_to(ceiling), FrameLimit::fps(30));
        assert_eq!(
            FrameLimit::unlimited().clamped_to(ceiling),
            FrameLimit::fps(60)
        );
    }

    /// **A value no frame rate can be read out of caps nothing and warns,
    /// naming the key.**
    ///
    /// A negative and a fraction are here beside the string because TOML
    /// spells both and neither is a `u32`: a reader written against `i64` would
    /// take `-1` and hand it on as a rate of four billion.
    #[test]
    fn a_frame_limit_that_is_not_a_usable_rate_warns_and_caps_nothing() {
        for value in ["\"sixty\"", "-1", "59.94", "true"] {
            let capture = crcbl_core::log::capture();
            let toml = format!("[{VIDEO_NAMESPACE}]\n{FRAME_LIMIT_KEY} = {value}\n");
            assert_eq!(
                ceiling_of(&toml),
                FrameLimit::unlimited(),
                "`{value}` was read as a rate"
            );

            let warned: Vec<_> = capture
                .records()
                .into_iter()
                .filter(|record| {
                    record
                        .message
                        .contains(&format!("{VIDEO_NAMESPACE}.{FRAME_LIMIT_KEY}"))
                })
                .collect();
            assert_eq!(
                warned.len(),
                1,
                "`{value}` should warn exactly once: {:?}",
                capture.records()
            );
            assert_eq!(warned[0].level, crcbl_core::log::Level::Warn);
        }
    }

    /// **An unlimited ceiling is written as a row, not left out.**
    ///
    /// [`an_effect_left_standing_is_still_a_row_in_the_file`]'s case for a
    /// number: a screen that omitted the row could never move a player's
    /// ceiling back off a cap they had already saved, because the key it would
    /// have to remove is the one it never writes.
    #[test]
    fn a_saved_frame_limit_reads_back_and_unlimited_is_a_row() {
        let (reloaded, written) = round_trip(|stack| {
            set_frame_limit(stack, FrameLimit::fps(30)).expect("a fresh user layer takes the key");
        });
        assert_eq!(frame_limit(&reloaded), FrameLimit::fps(30));
        assert!(
            written.contains(&format!("{FRAME_LIMIT_KEY} = 30")),
            "the cap is missing from the file:\n{written}"
        );

        let (reloaded, written) = round_trip(|stack| {
            set_frame_limit(stack, FrameLimit::unlimited())
                .expect("a fresh user layer takes the key");
        });
        assert_eq!(frame_limit(&reloaded), FrameLimit::unlimited());
        assert!(
            written.contains(&format!("{FRAME_LIMIT_KEY} = 0")),
            "an unlimited ceiling left no row behind:\n{written}"
        );
    }

    // ── The typed catalogue, `apply` and the console's variables ────────────

    /// A [`Stage`] that records what it was asked to apply, and answers
    /// [`Unsupported`] for the seam it does not have.
    ///
    /// A recorder rather than a real bundle because what these tests are about
    /// is what [`apply`] *asked for*: the renderer's own arithmetic is
    /// `crcbl-render`'s and needs a device, and asserting on it here would be
    /// asserting on the wrong half.
    #[derive(Debug, Default)]
    struct Recorder {
        video: Vec<VideoSettings>,
        gains: Vec<(Bus, f32)>,
        limits: Vec<FrameLimit>,
        /// Whether `set_frame_limit` has a clock behind it, so one test can turn
        /// the seam off and read [`Applied::NextStart`] back.
        has_clock: bool,
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
    }

    /// The values a kind's own ends are, for a sweep that has to touch both.
    fn ends_of(kind: Kind) -> Vec<Value> {
        match kind {
            Kind::Bool => vec![Value::Bool(false), Value::Bool(true)],
            Kind::Int { min, max } => vec![Value::Int(min), Value::Int(max)],
            Kind::Float { min, max } => vec![Value::Float(min), Value::Float(max)],
            Kind::Enum(values) => values.iter().copied().map(Value::Enum).collect(),
            Kind::Text => vec![Value::Text("anything".to_owned())],
        }
    }

    /// The binding the console reaches `name` through.
    fn binding_for(name: &str) -> &'static Binding {
        console_bindings()
            .iter()
            .copied()
            .find(|binding| binding.name() == name)
            .unwrap_or_else(|| panic!("`{name}` has no console binding"))
    }

    /// **Every numeric kind's own range is the range its setter stores**, at
    /// both ends.
    ///
    /// The failure this exists for is the one
    /// `docs/plan/52-debug-console.md` decision 3 names: a domain that says
    /// "1 to 16" while the setter clamps to something else, so a console
    /// accepts a value the file then reads back as a different one. Written as
    /// a sweep over [`catalogue`] rather than a list, so a key added with a
    /// hand-written range joins it the day it lands — and the count is
    /// asserted, because a sweep that matched nothing would pass in silence.
    #[test]
    fn every_kind_admits_the_ends_of_its_own_range_and_reads_them_back() {
        let mut checked = 0;
        for entry in catalogue() {
            if entry.status != KeyStatus::Read {
                continue;
            }
            for end in ends_of(entry.kind) {
                let binding = binding_for(entry.name);
                let mut host = ConsoleHost::new(stack_from(""));
                binding
                    .set(&mut host, &end)
                    .unwrap_or_else(|fault| panic!("`{}` refused {end}: {fault}", entry.key));
                assert_eq!(
                    binding.get(&host),
                    end,
                    "`{}` did not read back the value its own kind admits",
                    entry.key,
                );
                assert!(
                    host.stack().contains(&entry.key),
                    "`{}` was applied without being written",
                    entry.key,
                );
                checked += 1;
            }
        }
        // Six switches, four video rows and six gains, two ends each bar the
        // tier's three rungs.
        assert_eq!(checked, 33, "the sweep did not cover the read catalogue");
    }

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

    /// **A key nothing reads refuses a set, and says why.**
    ///
    /// The console half of [`KeyStatus::Named`]: a control that silently does
    /// nothing is worse than one that says so, which is what that enum exists
    /// for — and the binding carries [`Flags::READ_ONLY`] so the refusal
    /// happens before [`apply`] is even reached.
    #[test]
    fn a_key_nothing_reads_refuses_a_set_through_both_doors() {
        let key = format!("{VIDEO_NAMESPACE}.fov");
        let mut stack = stack_from("");
        let mut stage = Recorder::default();
        let fault = apply(&mut stack, &key, &Value::Float(90.0), &mut stage)
            .expect_err("nothing reads the field of view");
        assert!(
            fault.message().contains("nothing reads this yet"),
            "{}",
            fault.message()
        );
        assert!(!stack.contains(&key), "a refused write reached the file");

        let binding = binding_for("fov");
        assert!(binding.flags().contains(Flags::READ_ONLY));
        let mut host = ConsoleHost::new(stack_from(""));
        assert_eq!(
            binding
                .set(&mut host, &Value::Float(90.0))
                .expect_err("read only")
                .message(),
            "`fov` is read-only"
        );
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
            apply(&mut stack, &key, &Value::Enum("smaa"), &mut stage),
            Ok(Applied::Live)
        );
        assert_eq!(
            stage
                .video
                .last()
                .expect("the renderer was told")
                .antialiasing,
            Some(Antialiasing::Smaa)
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
        assert_eq!(video.antialiasing, Some(Antialiasing::Smaa));

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

    /// **A write the console makes is waiting for the frame that can show it.**
    ///
    /// The console's host cannot hold the renderer — see [`Deferred`] — so the
    /// claim that has to hold is that the ask survives to be drained, and that
    /// the drain empties it.
    #[test]
    fn a_console_write_is_recorded_for_the_frame_to_drain() {
        let binding = binding_for(RENDER_SCALE_KEY);
        let mut host = ConsoleHost::new(stack_from(""));
        assert!(host.pending_mut().is_empty());

        binding
            .set(&mut host, &Value::Float(0.5))
            .expect("inside the range");
        let taken = host.pending_mut().take_video().expect("the frame has work");
        assert!((taken.render_scale - 0.5).abs() < f32::EPSILON);
        assert!(
            host.pending_mut().is_empty(),
            "the drain left the ask behind, so the next frame would apply it again"
        );
    }

    /// **There is one console variable per catalogue key, under the key's own
    /// bare name.**
    ///
    /// Both directions, because either alone passes on an empty list: the count
    /// against [`catalogue`], and every binding's name back to a catalogue
    /// entry with the same kind and help.
    #[test]
    fn the_bindings_are_the_catalogue() {
        let catalogue = catalogue();
        assert_eq!(
            console_bindings().len(),
            catalogue.len(),
            "the macro's list and the catalogue disagree about how many keys there are",
        );
        for binding in console_bindings() {
            let entry = catalogue
                .iter()
                .find(|entry| entry.name == binding.name())
                .unwrap_or_else(|| panic!("`{}` is a variable and not a key", binding.name()));
            assert_eq!(binding.kind(), entry.kind, "`{}`", entry.key);
            assert_eq!(binding.help(), entry.help, "`{}`", entry.key);
            assert!(
                binding.flags().contains(Flags::ARCHIVE),
                "`{}` is not persisted, though the settings stack is its storage",
                entry.key,
            );
            assert_eq!(
                binding.flags().contains(Flags::READ_ONLY),
                entry.status == KeyStatus::Named,
                "`{}`'s console flag disagrees with its catalogue status",
                entry.key,
            );
        }
    }

    /// **A `READ_ONLY` binding still prints**, which is the point of listing the
    /// half of the catalogue nothing reads.
    #[test]
    fn a_key_nothing_reads_still_prints_what_the_file_holds() {
        let host = ConsoleHost::new(stack_from("[engine.video]\nfov = 75.0\n"));
        assert_eq!(binding_for("fov").get(&host), Value::Float(75.0));
        assert_eq!(
            binding_for("display_mode").get(&host),
            Value::Enum("windowed"),
            "an absent enum reads back as the first name in its set"
        );
    }

    /// **Every debug view turns on exactly the switch
    /// [`crcbl_render::ForwardRenderer::debug_view`] reads it back off**, and
    /// leaves the other four alone.
    ///
    /// The renderer needs a device and these do not, so this asserts the table
    /// against that function's precedence order directly: motion, occlusion,
    /// heatmap, LOD, normals. A view that set two switches would be drawn as
    /// whichever is outermost, silently.
    #[test]
    fn every_debug_view_sets_exactly_the_switch_its_precedence_reads_back() {
        use crcbl_render::DebugView as V;
        let order = [
            V::Motion,
            V::AmbientOcclusion,
            V::Heatmap,
            V::LodTint,
            V::Normals,
        ];
        assert_eq!(
            debug_view_switches(V::Shaded),
            [false; 5],
            "the shaded frame is every switch off",
        );
        for (index, view) in order.into_iter().enumerate() {
            let switches = debug_view_switches(view);
            assert_eq!(
                switches.iter().filter(|on| **on).count(),
                1,
                "{view:?} sets more than one switch",
            );
            assert!(
                switches[index],
                "{view:?} is not at its precedence position"
            );
        }
    }
}
