//! The `[engine.video]` section: its keys, the readers that answer them and
//! the writers that store them. The [module docs](super) say what each kind of
//! key may do to a frame.

use crcbl_console::{Kind, Value};
use crcbl_render::{
    Antialiasing, DEFAULT_ANISOTROPY, MIN_RENDER_SCALE, RenderEffects, r_ssao_bent_normals,
    r_ssao_blur_passes, r_ssao_slices,
    shadow::{Filter, shipped_filter},
};

use crate::engine::FrameLimit;
use crcbl_store::StorageError;
use crcbl_store::settings::SettingsStack;

#[cfg(doc)]
use super::presets;

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

/// The effect switches a quality tier writes and a settings screen does not
/// show: [`VIDEO_KEYS`]' twin for an effect topic 45 made "not a settings row of
/// its own but a tier item" (2026-08-30, `docs/notes/rendering.md`).
///
/// Read, written, catalogued and bound to the console exactly as
/// [`VIDEO_KEYS`] are — a preset clears a bit by writing its key, and a bit
/// with no key is one no preset can reach — and absent from that table only
/// because `apps/options` builds its effect rows from it.
pub const TIER_VIDEO_KEYS: [(&str, RenderEffects); 1] =
    [("contact_shadows", RenderEffects::CONTACT_SHADOWS)];

/// Every effect switch of `[engine.video]`: [`VIDEO_KEYS`], then
/// [`TIER_VIDEO_KEYS`].
pub(super) fn effect_keys() -> impl Iterator<Item = (&'static str, RenderEffects)> {
    VIDEO_KEYS.into_iter().chain(TIER_VIDEO_KEYS)
}

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
    for (key, effect) in effect_keys() {
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

/// The `[engine.video]` key that multiplies the UI's scale.
///
/// Spelled here for [`RENDER_SCALE_KEY`]'s reason, and read by [`ui_scale`].
pub const UI_SCALE_KEY: &str = "ui_scale";

/// The smallest multiplier [`ui_scale`] reads: a quarter of the base scale.
pub const MIN_UI_SCALE: f32 = 0.25;

/// The largest multiplier [`ui_scale`] reads: four times the base scale.
pub const MAX_UI_SCALE: f32 = 4.0;

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

/// The `[engine.video]` key that picks the shadow filter.
pub const SHADOW_FILTER_KEY: &str = "shadow_filter";

/// The `[engine.video]` key that sets the SSAO slice count.
pub const SSAO_SLICES_KEY: &str = "ssao_slices";
/// The `[engine.video]` key that sets the SSAO blur-pass count.
pub const SSAO_BLUR_PASSES_KEY: &str = "ssao_blur_passes";
/// The `[engine.video]` key that controls SSAO bent-normal output.
pub const SSAO_BENT_NORMALS_KEY: &str = "ssao_bent_normals";

/// The SSAO convar domains, shared by the settings catalogue and writers.
pub(super) const SSAO_SLICES_KIND: Kind = Kind::Int { min: 2, max: 4 };
pub(super) const SSAO_BLUR_PASSES_KIND: Kind = Kind::Int { min: 1, max: 2 };
pub(super) const SSAO_BENT_NORMALS_KIND: Kind = Kind::Bool;

/// Every rung [`shadow_filter`] reads, as the words a file and a console line
/// spell them with.
pub const SHADOW_FILTER_NAMES: [&str; Filter::ALL.len()] = {
    let mut names = [""; Filter::ALL.len()];
    let mut i = 0;
    while i < Filter::ALL.len() {
        names[i] = Filter::ALL[i].label();
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
/// clamp, not this one; see the [module docs](super).
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
    /// resolve slot rather than clamping it — see the [module docs](super).
    pub antialiasing: Option<Antialiasing>,
    /// Which shadow filter the player picked; see [`shadow_filter`].
    pub shadow_filter: Filter,
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
    /// The SSAO slice count; see [`ssao_slices`].
    pub ssao_slices: i64,
    /// The number of SSAO blur passes; see [`ssao_blur_passes`].
    pub ssao_blur_passes: i64,
    /// Whether SSAO writes bent normals; see [`ssao_bent_normals`].
    pub ssao_bent_normals: bool,
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
            shadow_filter: shipped_filter(),
            render_scale: 1.0,
            anisotropic_filtering: DEFAULT_ANISOTROPY,
            frame_limit: FrameLimit::unlimited(),
            ssao_slices: ssao_slices_default(),
            ssao_blur_passes: ssao_blur_passes_default(),
            ssao_bent_normals: ssao_bent_normals_default(),
        }
    }
}

/// The whole `[engine.video]` section, off one stack.
#[must_use]
pub fn video(stack: &SettingsStack) -> VideoSettings {
    VideoSettings {
        effects: video_effects(stack),
        antialiasing: antialiasing(stack),
        shadow_filter: shadow_filter(stack),
        render_scale: render_scale(stack),
        anisotropic_filtering: anisotropic_filtering(stack),
        frame_limit: frame_limit(stack),
        ssao_slices: ssao_slices(stack),
        ssao_blur_passes: ssao_blur_passes(stack),
        ssao_bent_normals: ssao_bent_normals(stack),
    }
}

/// Which antialiasing tier the player picked, for
/// [`EffectRequest::antialiasing`](crcbl_render::EffectRequest::antialiasing).
///
/// [`None`] for a stack that says nothing, which leaves the view's own stack
/// holding the resolve slot, and otherwise the [`Antialiasing`] rung the key
/// names — `"none"`, `"fxaa"` or `"cmaa2"`, [`Antialiasing::name`]'s spelling on
/// both sides of the round trip.
///
/// # A file still holding the boolean reads as one of two things
///
/// The key was a `bool` beside a second key called `smaa` until
/// the antialiasing ladder's eighth decision (`docs/notes/rendering.md`)
/// folded the pair into this ladder. There is no migration — everything here is v0 — but the two spellings
/// a hand-edited file can still hold are answered rather than warned about,
/// because both had a meaning and neither is a mistake the player made:
/// `antialiasing = true` was "the player has not asked for less", which is
/// exactly [`None`] here, and `antialiasing = false` was "no resolve at all",
/// which is [`Antialiasing::None`]. A `smaa` key is not read by anything and is
/// reported by `crcbl settings list` as a key the engine does not define — and
/// so is the *value* `"smaa"`, which was the higher rung's word until
/// the CMAA2 slice retired that tier. It is now a
/// word no rung wears, and the paragraph below is what a file holding it gets:
/// an unpicked tier and one warning naming the key.
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

/// Which shadow filter the player picked.
///
/// The shipped filter for a stack that says nothing, and otherwise the filter
/// the key names. A present value outside the domain or of the wrong type falls
/// back to the shipped filter and warns once, naming the key.
#[must_use]
pub fn shadow_filter(stack: &SettingsStack) -> Filter {
    let dotted = format!("{VIDEO_NAMESPACE}.{SHADOW_FILTER_KEY}");
    let unreadable = || {
        crcbl_core::log::warn!(
            "settings: `{dotted}` names no shadow filter, so it does nothing; \
             the frame uses the shipped filter"
        );
        shipped_filter()
    };
    match stack.get::<String>(&dotted) {
        Some(name) if SHADOW_FILTER_NAMES.contains(&name.as_str()) => {
            Filter::from_name(&name).expect("the settings domain is derived from `Filter::ALL`")
        }
        Some(_) => unreadable(),
        None if stack.contains(&dotted) => unreadable(),
        None => shipped_filter(),
    }
}

pub(super) fn ssao_slices_default() -> i64 {
    let Value::Int(default) = r_ssao_slices.default() else {
        unreachable!("`r_ssao_slices` is declared as an integer")
    };
    *default
}

pub(super) fn ssao_blur_passes_default() -> i64 {
    let Value::Int(default) = r_ssao_blur_passes.default() else {
        unreachable!("`r_ssao_blur_passes` is declared as an integer")
    };
    *default
}

pub(super) fn ssao_bent_normals_default() -> bool {
    let Value::Bool(default) = r_ssao_bent_normals.default() else {
        unreachable!("`r_ssao_bent_normals` is declared as a bool")
    };
    *default
}

/// Read one bounded SSAO integer, falling back to its convar's declared default.
fn ssao_integer(stack: &SettingsStack, key: &str, kind: Kind, default: i64) -> i64 {
    let dotted = format!("{VIDEO_NAMESPACE}.{key}");
    match stack.get::<i64>(&dotted) {
        Some(value) if kind.check(&dotted, &Value::Int(value)).is_ok() => value,
        Some(_) => {
            crcbl_core::log::warn!(
                "settings: `{dotted}` is outside its SSAO domain, so it does nothing; the frame uses the shipped value"
            );
            default
        }
        None if stack.contains(&dotted) => {
            crcbl_core::log::warn!(
                "settings: `{dotted}` is not a usable SSAO value, so it does nothing; the frame uses the shipped value"
            );
            default
        }
        None => default,
    }
}

/// The SSAO slice count; absent or invalid values use the convar's declared default.
#[must_use]
pub fn ssao_slices(stack: &SettingsStack) -> i64 {
    ssao_integer(
        stack,
        SSAO_SLICES_KEY,
        SSAO_SLICES_KIND,
        ssao_slices_default(),
    )
}

/// The SSAO blur-pass count; absent or invalid values use the convar's declared default.
#[must_use]
pub fn ssao_blur_passes(stack: &SettingsStack) -> i64 {
    ssao_integer(
        stack,
        SSAO_BLUR_PASSES_KEY,
        SSAO_BLUR_PASSES_KIND,
        ssao_blur_passes_default(),
    )
}

/// Whether SSAO writes bent normals; absent or invalid values use the convar's declared default.
#[must_use]
pub fn ssao_bent_normals(stack: &SettingsStack) -> bool {
    let default = ssao_bent_normals_default();
    let dotted = format!("{VIDEO_NAMESPACE}.{SSAO_BENT_NORMALS_KEY}");
    match stack.get::<bool>(&dotted) {
        Some(value) => value,
        None if stack.contains(&dotted) => {
            crcbl_core::log::warn!(
                "settings: `{dotted}` is not true or false, so it does nothing; the frame uses the shipped value"
            );
            default
        }
        None => default,
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

/// The multiplier the player puts over the UI's base scale, for
/// [`DrawList::set_scale`](crcbl_ui::draw_list::DrawList::set_scale).
///
/// The base is the host's to choose — the window's own scale factor
/// ([`crate::ui_scale::window_scale_factor`]) or a game's fit to a reference
/// window ([`crate::ui_scale::fit_scale`]) — and the list is drawn at the base
/// times this. `1.0` for a stack that says nothing, and otherwise the file's
/// value clamped to `MIN_UI_SCALE..=MAX_UI_SCALE`.
///
/// **Not in [`VideoSettings`]**, which is what the renderer is handed: nothing
/// in a renderer draws the UI at a scale, so the host that lays the UI out
/// reads this itself.
///
/// # A line that does nothing says so
///
/// On [`render_scale`]'s terms exactly: a value this cannot use — a string,
/// `nan`, `inf` — leaves the multiplier at `1.0` and **warns**, naming the key,
/// and a finite value outside the range is clamped without a word.
#[must_use]
pub fn ui_scale(stack: &SettingsStack) -> f32 {
    let dotted = format!("{VIDEO_NAMESPACE}.{UI_SCALE_KEY}");
    let unreadable = || {
        crcbl_core::log::warn!(
            "settings: `{dotted}` is not a usable number, so it does nothing; \
             the UI is drawn at its base scale"
        );
        1.0
    };
    match stack.get::<f64>(&dotted) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "clamped to [MIN_UI_SCALE, MAX_UI_SCALE], where every f64 has an f32 within an ulp"
        )]
        // The finite check first, for `render_scale`'s reason: `clamp` answers
        // NaN for NaN, and a NaN scale would draw no UI at all.
        Some(scale) if scale.is_finite() => (scale as f32).clamp(MIN_UI_SCALE, MAX_UI_SCALE),
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
/// it refuses. The [module docs](super) say why this is the one key that may
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

/// Which tier the frame resolves with, for a caller that has no word for
/// "unpicked".
///
/// [`antialiasing`]'s answer where the player named a rung, and otherwise the
/// rung [`RenderEffects::DEFAULT_STACK`] carries — which is what a view's own
/// stack keeps when the file says nothing. The console's `antialiasing` row and
/// [`presets::selected`] both read it rather than each spelling the fallback,
/// because a row that showed one rung while a preset compared against another
/// would make the label disagree with the value beside it.
pub(super) fn antialiasing_or_default(stack: &SettingsStack) -> Antialiasing {
    antialiasing(stack).unwrap_or_else(|| Antialiasing::from_effects(RenderEffects::DEFAULT_STACK))
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
    set_shadow_filter(stack, video.shadow_filter)?;
    set_render_scale(stack, video.render_scale)?;
    set_anisotropic_filtering(stack, video.anisotropic_filtering)?;
    set_frame_limit(stack, video.frame_limit)?;
    set_ssao_slices(stack, video.ssao_slices)?;
    set_ssao_blur_passes(stack, video.ssao_blur_passes)?;
    set_ssao_bent_normals(stack, video.ssao_bent_normals)
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

/// Write `[engine.video] shadow_filter`, as the rung [`shadow_filter`] reads back.
pub fn set_shadow_filter(stack: &mut SettingsStack, filter: Filter) -> Result<(), StorageError> {
    stack.set(
        &format!("{VIDEO_NAMESPACE}.{SHADOW_FILTER_KEY}"),
        &filter.label(),
    )
}

/// Write the effect rows of `[engine.video]`, one key per [`VIDEO_KEYS`] and
/// [`TIER_VIDEO_KEYS`] entry.
///
/// # Errors
///
/// [`set_video`]'s.
pub fn set_video_effects(
    stack: &mut SettingsStack,
    allowed: RenderEffects,
) -> Result<(), StorageError> {
    for (key, effect) in effect_keys() {
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

/// Write `[engine.video] ui_scale`, clamped to what [`ui_scale`] reads.
///
/// Clamped on the way in on [`set_render_scale`]'s terms, so the file holds the
/// multiplier the next read will actually answer.
///
/// # Errors
///
/// [`set_video`]'s, and a value that is not finite, for [`set_render_scale`]'s
/// reason.
pub fn set_ui_scale(stack: &mut SettingsStack, scale: f32) -> Result<(), StorageError> {
    let dotted = format!("{VIDEO_NAMESPACE}.{UI_SCALE_KEY}");
    if !scale.is_finite() {
        return Err(StorageError::Other(format!(
            "settings: `{dotted}` cannot be written as {scale}"
        )));
    }
    stack.set(&dotted, &f64::from(scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE)))
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

/// Write `[engine.video] ssao_slices`, rejecting values outside the convar's domain.
pub fn set_ssao_slices(stack: &mut SettingsStack, slices: i64) -> Result<(), StorageError> {
    let dotted = format!("{VIDEO_NAMESPACE}.{SSAO_SLICES_KEY}");
    if SSAO_SLICES_KIND
        .check(&dotted, &Value::Int(slices))
        .is_err()
    {
        return Err(StorageError::Other(format!(
            "settings: `{dotted}` cannot be written as {slices}"
        )));
    }
    stack.set(&dotted, &slices)
}

/// Write `[engine.video] ssao_blur_passes`, rejecting values outside the convar's domain.
pub fn set_ssao_blur_passes(stack: &mut SettingsStack, passes: i64) -> Result<(), StorageError> {
    let dotted = format!("{VIDEO_NAMESPACE}.{SSAO_BLUR_PASSES_KEY}");
    if SSAO_BLUR_PASSES_KIND
        .check(&dotted, &Value::Int(passes))
        .is_err()
    {
        return Err(StorageError::Other(format!(
            "settings: `{dotted}` cannot be written as {passes}"
        )));
    }
    stack.set(&dotted, &passes)
}

/// Write `[engine.video] ssao_bent_normals`.
pub fn set_ssao_bent_normals(
    stack: &mut SettingsStack,
    bent_normals: bool,
) -> Result<(), StorageError> {
    stack.set(
        &format!("{VIDEO_NAMESPACE}.{SSAO_BENT_NORMALS_KEY}"),
        &bent_normals,
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

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_store::MemoryStorage;
    use crcbl_store::StorageSource;
    use crcbl_store::settings::SETTINGS_FILE;

    use crate::settings::catalogued;
    use crate::settings::tests::{round_trip, stack_from};

    /// **What a settings screen writes is what the next start-up reads.**
    ///
    /// Every field of [`VideoSettings`] at once, and none of them at its
    /// default: a round trip that carried nothing would still pass if the
    /// values under test were the ones an empty file already answers.
    #[test]
    fn a_saved_video_section_reads_back_unchanged() {
        let wanted = VideoSettings {
            effects: RenderEffects::all() - RenderEffects::BLOOM - RenderEffects::SHADOWS,
            antialiasing: Some(Antialiasing::Cmaa2),
            shadow_filter: Filter::Disc,
            render_scale: 0.5,
            anisotropic_filtering: 4.0,
            frame_limit: FrameLimit::fps(60),
            ssao_slices: 2,
            ssao_blur_passes: 1,
            ssao_bent_normals: false,
        };
        let (reloaded, _) = round_trip(|stack| {
            set_video(stack, wanted).expect("a fresh user layer accepts every key");
        });
        assert_eq!(video(&reloaded), wanted);
    }

    /// The settings domains and fallbacks are the renderer convars' declarations.
    #[test]
    fn ssao_catalogue_kinds_and_defaults_match_the_renderer_convars() {
        for (key, kind, convar) in [
            (SSAO_SLICES_KEY, SSAO_SLICES_KIND, &r_ssao_slices),
            (
                SSAO_BLUR_PASSES_KEY,
                SSAO_BLUR_PASSES_KIND,
                &r_ssao_blur_passes,
            ),
            (
                SSAO_BENT_NORMALS_KEY,
                SSAO_BENT_NORMALS_KIND,
                &r_ssao_bent_normals,
            ),
        ] {
            assert_eq!(kind, convar.kind(), "{key}");
            assert_eq!(
                catalogued(&format!("{VIDEO_NAMESPACE}.{key}"))
                    .expect("catalogued")
                    .kind,
                convar.kind(),
                "{key}",
            );
        }
        assert_eq!(ssao_slices(&stack_from("")), ssao_slices_default());
        assert_eq!(
            ssao_blur_passes(&stack_from("")),
            ssao_blur_passes_default()
        );
        assert_eq!(
            ssao_bent_normals(&stack_from("")),
            ssao_bent_normals_default()
        );
    }

    /// SSAO settings persist both integer ends and both boolean values.
    #[test]
    fn ssao_settings_round_trip_all_ends_and_bool() {
        for slices in [2, 4] {
            for passes in [1, 2] {
                for bent_normals in [false, true] {
                    let (reloaded, _) = round_trip(|stack| {
                        set_ssao_slices(stack, slices).expect("valid slices");
                        set_ssao_blur_passes(stack, passes).expect("valid passes");
                        set_ssao_bent_normals(stack, bent_normals).expect("valid bool");
                    });
                    assert_eq!(ssao_slices(&reloaded), slices);
                    assert_eq!(ssao_blur_passes(&reloaded), passes);
                    assert_eq!(ssao_bent_normals(&reloaded), bent_normals);
                }
            }
        }
    }

    /// Invalid and wrong-type SSAO settings warn once and use convar defaults.
    #[test]
    fn invalid_ssao_settings_warn_once_and_use_convar_defaults() {
        for (key, value) in [
            (SSAO_SLICES_KEY, "9"),
            (SSAO_SLICES_KEY, "\"many\""),
            (SSAO_BLUR_PASSES_KEY, "0"),
            (SSAO_BLUR_PASSES_KEY, "false"),
            (SSAO_BENT_NORMALS_KEY, "\"yes\""),
        ] {
            let capture = crcbl_core::log::capture();
            let stack = stack_from(&format!("[{VIDEO_NAMESPACE}]\n{key} = {value}\n"));
            assert_eq!(ssao_slices(&stack), ssao_slices_default());
            assert_eq!(ssao_blur_passes(&stack), ssao_blur_passes_default());
            assert_eq!(ssao_bent_normals(&stack), ssao_bent_normals_default());
            let warned: Vec<_> = capture
                .records()
                .into_iter()
                .filter(|record| record.message.contains(&format!("{VIDEO_NAMESPACE}.{key}")))
                .collect();
            assert_eq!(warned.len(), 1, "{key}: {:?}", capture.records());
        }
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

    /// **Every effect is reachable from a settings file**, through the boolean
    /// table or through the antialiasing ladder, and no two keys claim one bit.
    ///
    /// The guard against a bit added to [`RenderEffects`] and not to
    /// [`VIDEO_KEYS`]: the omission has no symptom of its own — the effect
    /// simply cannot be turned off, and a player's row does nothing — so
    /// nothing else would report it. The two resolve bits are the exception the
    /// ladder exists for, so they are named here as the ladder's and asserted to
    /// be **out** of the boolean table: a `cmaa2 = false` row a player could
    /// still write is a row nothing reads.
    ///
    /// # Contact shadows have a key and no menu row
    ///
    /// Topic 45's 2026-08-30 decision made
    /// [`RenderEffects::CONTACT_SHADOWS`] "not a settings row of its own but a
    /// tier item", and the low preset clears it; a preset clears a bit by
    /// writing its key, so the bit has one in [`TIER_VIDEO_KEYS`], which
    /// `apps/options` does not draw rows from. It is covered here like any
    /// other switch.
    #[test]
    fn every_effect_has_a_key_and_no_two_share_one() {
        let mut covered = RenderEffects::empty();
        for (key, effect) in effect_keys() {
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
            "an effect with no [engine.video] key, no ladder rung and no place in the tier \
             set is one nothing can reach"
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

    /// The UI multiplier the reader answers off a file holding `toml`.
    fn ui_scale_of(toml: &str) -> f32 {
        ui_scale(&stack_from(toml))
    }

    /// **The UI multiplier is one when the file says nothing, and otherwise
    /// the file's value clamped to the catalogue's range** — both ends, and a
    /// whole number read as a multiplier.
    #[test]
    fn a_ui_scale_is_read_and_clamped_to_its_range() {
        assert_eq!(ui_scale_of(""), 1.0);
        let key = format!("[{VIDEO_NAMESPACE}]\n{UI_SCALE_KEY} = ");
        assert_eq!(ui_scale_of(&format!("{key}1.25\n")), 1.25);
        assert_eq!(ui_scale_of(&format!("{key}2\n")), 2.0);
        assert_eq!(ui_scale_of(&format!("{key}9.0\n")), MAX_UI_SCALE);
        assert_eq!(ui_scale_of(&format!("{key}0.1\n")), MIN_UI_SCALE);
        assert_eq!(ui_scale_of(&format!("{key}-1.0\n")), MIN_UI_SCALE);
        let Kind::Float { min, max } = catalogued(&format!("{VIDEO_NAMESPACE}.{UI_SCALE_KEY}"))
            .expect("the UI scale is catalogued")
            .kind
        else {
            panic!("the UI scale is not a float kind");
        };
        assert_eq!((min, max), (MIN_UI_SCALE, MAX_UI_SCALE));
    }

    /// **A UI multiplier that is not a usable number is one, and warns once,
    /// naming the key.**
    #[test]
    fn a_ui_scale_that_is_not_a_usable_number_warns_and_is_one() {
        let dotted = format!("{VIDEO_NAMESPACE}.{UI_SCALE_KEY}");
        for value in ["\"big\"", "nan", "inf", "-inf"] {
            let capture = crcbl_core::log::capture();
            let scale = ui_scale_of(&format!("[{VIDEO_NAMESPACE}]\n{UI_SCALE_KEY} = {value}\n"));
            assert_eq!(scale, 1.0, "`{value}` was read as {scale}");
            let warned: Vec<_> = capture
                .records()
                .into_iter()
                .filter(|record| record.message.contains(&dotted))
                .collect();
            assert_eq!(warned.len(), 1, "`{value}`: {:?}", capture.records());
            assert_eq!(warned[0].level, crcbl_core::log::Level::Warn);
        }
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
    /// screen that saves `"cmaa2"` and a start-up that reads back `"none"` — the
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
    /// The spellings are the ones a hand-edited file plausibly holds: the word
    /// of a rung that is **no longer** on the ladder — `smaa`, which was the
    /// higher tier until the CMAA2 slice retired
    /// it — the same word in the wrong case, and the numbers TOML would take for
    /// the boolean this key used to be.
    #[test]
    fn an_antialiasing_key_naming_no_rung_warns_and_picks_nothing() {
        for value in ["\"smaa\"", "\"FXAA\"", "\"\"", "1", "0.5"] {
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

    /// **Every shadow-filter rung round trips through a file**, under the one
    /// spelling [`Filter::label`] gives it.
    #[test]
    fn every_shadow_filter_rung_round_trips_through_a_file() {
        for filter in Filter::ALL {
            let (reloaded, written) = round_trip(|stack| {
                set_shadow_filter(stack, filter).expect("a fresh user layer takes the key");
            });
            assert_eq!(shadow_filter(&reloaded), filter);
            assert!(
                written.contains(&format!("{SHADOW_FILTER_KEY} = \"{}\"", filter.label())),
                "{filter:?} left no row behind:\n{written}"
            );
        }
    }

    /// **A shadow-filter value outside its domain or of the wrong type falls
    /// back to shipped PCSS and warns once.**
    #[test]
    fn an_invalid_shadow_filter_warns_and_uses_shipped_pcss() {
        for value in ["\"soft\"", "true", "4"] {
            let capture = crcbl_core::log::capture();
            let toml = format!("[{VIDEO_NAMESPACE}]\n{SHADOW_FILTER_KEY} = {value}\n");
            assert_eq!(shadow_filter(&stack_from(&toml)), Filter::Pcss);
            let warned: Vec<_> = capture
                .records()
                .into_iter()
                .filter(|record| {
                    record
                        .message
                        .contains(&format!("{VIDEO_NAMESPACE}.{SHADOW_FILTER_KEY}"))
                })
                .collect();
            assert_eq!(warned.len(), 1, "`{value}`: {:?}", capture.records());
            assert_eq!(warned[0].level, crcbl_core::log::Level::Warn);
        }
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
