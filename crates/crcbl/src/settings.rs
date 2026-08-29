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

use crcbl_audio::mixer::Bus;
use crcbl_render::{DEFAULT_ANISOTROPY, MIN_RENDER_SCALE, RenderEffects};

use crate::engine::FrameLimit;
use crcbl_store::StorageError;
use crcbl_store::settings::SettingsStack;

/// The `[engine.video]` section, as a dotted key prefix.
pub const VIDEO_NAMESPACE: &str = "engine.video";

/// Every effect a player can switch off, and the `[engine.video]` key that does
/// it.
///
/// **The one place a key is spelled.** A settings screen writing the row and a
/// start-up reading it back go through this table, because two spellings of one
/// key is a game that saves a setting it will never load again.
///
/// The names are the ones `crcbl_store::settings`' own examples already use for
/// this namespace — bare snake_case nouns beside `vsync` and `master_volume` —
/// rather than the flag spellings (`no_shadows`, `enable_ssao`) the same
/// switches have on a command line. A settings file is a description of what
/// the player wants on, and a negated key would make `shadows = false` and
/// `no_shadows = false` both writable and opposite.
pub const VIDEO_KEYS: [(&str, RenderEffects); 7] = [
    ("shadows", RenderEffects::SHADOWS),
    ("ambient_occlusion", RenderEffects::AMBIENT_OCCLUSION),
    ("reflections", RenderEffects::REFLECTIONS),
    ("bloom", RenderEffects::BLOOM),
    ("antialiasing", RenderEffects::ANTIALIASING),
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
    /// What a player who has said nothing gets: every effect standing, the
    /// full extent and the page at the engine's own anisotropy.
    ///
    /// Also what [`SettingsSource::None`](crate::engine::SettingsSource::None)
    /// answers, and the two are the same answer for the same reason — this
    /// layer may only take away, so "nothing to read" and "nothing taken away"
    /// cannot differ.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            effects: RenderEffects::all(),
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
        render_scale: render_scale(stack),
        anisotropic_filtering: anisotropic_filtering(stack),
        frame_limit: frame_limit(stack),
    }
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
/// [`RENDER_SCALE_KEY`], [`ANISOTROPIC_FILTERING_KEY`] and [`FRAME_LIMIT_KEY`].
/// Nothing is persisted until the
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
/// # Errors
///
/// [`SettingsStack::set`]'s: no user layer in the stack, or an ancestor of a
/// key already holding a scalar in a hand-edited file.
pub fn set_video(stack: &mut SettingsStack, video: VideoSettings) -> Result<(), StorageError> {
    set_video_effects(stack, video.effects)?;
    set_render_scale(stack, video.render_scale)?;
    set_anisotropic_filtering(stack, video.anisotropic_filtering)?;
    set_frame_limit(stack, video.frame_limit)
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueKey {
    /// The dotted key, as it is written in `settings.toml` and passed to
    /// [`SettingsStack::get`].
    pub key: String,
    /// What the key accepts, for a screen to render and a person to read.
    pub domain: &'static str,
    /// Whether a reader answers it; see [`KeyStatus`].
    pub status: KeyStatus,
}

/// The `[engine.video]` rows nothing reads yet, with the domains
/// `docs/plan/15-windowing.md` fixed for them.
///
/// Literals, unlike the rows below them, because there is nothing in the tree
/// to derive them from — that is exactly what makes them [`KeyStatus::Named`].
/// A row leaves this list by growing a reader and joining `catalogue`'s derived
/// half, so the two halves cannot both claim one key.
const NAMED_VIDEO_KEYS: [(&str, &str); 8] = [
    ("display_mode", r#""windowed" | "borderless""#),
    (
        "monitor",
        "monitor name; absent means wherever the window is",
    ),
    ("resolution", "[width, height] in device pixels"),
    ("present_mode", r#""auto" | "vsync" | "adaptive" | "off""#),
    (
        "brightness",
        "scalar multiplier applied in the tonemap pass",
    ),
    ("hdr_output", "true | false"),
    ("ui_scale", "multiplier over the window's own scale factor"),
    ("fov", "vertical field of view in degrees"),
];

/// Every key the engine defines, read or merely named.
///
/// **Derived from the readers wherever there is a reader**, so a key cannot
/// appear here under one spelling and be read under another: the effect rows
/// come from [`VIDEO_KEYS`], the scale row from [`RENDER_SCALE_KEY`], the
/// anisotropy row from [`ANISOTROPIC_FILTERING_KEY`], and the volume rows from
/// [`Bus::settings_key`]. Only the rows with no reader are
/// written out, because there is nothing to derive them from.
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
    let read = |key: String, domain| CatalogueKey {
        key,
        domain,
        status: KeyStatus::Read,
    };
    let mut keys: Vec<CatalogueKey> = VIDEO_KEYS
        .iter()
        .map(|(key, _)| read(format!("{VIDEO_NAMESPACE}.{key}"), "true | false"))
        .collect();
    keys.push(read(
        format!("{VIDEO_NAMESPACE}.{RENDER_SCALE_KEY}"),
        "fraction of the surface extent; clamped to the renderer's floor",
    ));
    keys.push(read(
        format!("{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}"),
        "1 to 16; 1 is off, and the device's own ceiling clamps it",
    ));
    keys.push(read(
        format!("{VIDEO_NAMESPACE}.{FRAME_LIMIT_KEY}"),
        "frames a second; 0 is unlimited",
    ));
    keys.extend(NAMED_VIDEO_KEYS.map(|(key, domain)| CatalogueKey {
        key: format!("{VIDEO_NAMESPACE}.{key}"),
        domain,
        status: KeyStatus::Named,
    }));
    keys.extend(Bus::ALL.map(|bus| {
        read(
            format!("{AUDIO_NAMESPACE}.{}", bus.settings_key()),
            "gain from 0 to 1; absent is unity",
        )
    }));
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
            assert!(!entry.domain.is_empty(), "`{}` has no domain", entry.key);
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

    /// **Every effect can be switched off from a settings file.**
    ///
    /// The guard against a bit added to [`RenderEffects`] and not to
    /// [`VIDEO_KEYS`]: the omission has no symptom of its own — the effect
    /// simply cannot be turned off, and a player's row does nothing — so
    /// nothing else would report it.
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
        assert_eq!(
            covered,
            RenderEffects::all(),
            "an effect with no [engine.video] key is one a player cannot switch off"
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
            ("antialiasing", RenderEffects::ANTIALIASING),
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
}
