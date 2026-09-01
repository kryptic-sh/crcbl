//! Which of `docs/plan/18-render-features.md`'s effects a frame draws, and
//! `docs/plan/39-capabilities.md`'s four-layer order that decides it.
//!
//! ```text
//! camera stack (RON) declares what the view wants
//!   → [engine.video] clamps it downward as a player quality setting
//!       and its `antialiasing` rung replaces the one resolve slot
//!   → programmatic override may set it either way
//!   → device capability clamps it downward, last and absolutely
//! ```
//!
//! [`EffectRequest`] carries the first four — they are what a caller supplies
//! — and [`EffectRequest::resolve`] applies the whole order in one place, which
//! is the property topic 39 asks for by name. The renderer holds one request and
//! one resolved set, and every pass it records reads the resolved set; nothing
//! else in this crate branches on a layer.
//!
//! # The antialiasing rung is a choice, not a clamp
//!
//! [`Antialiasing`] is the one thing the `[engine.video]` layer **replaces**
//! rather than removes, and that type says why: a clamp can only take the
//! resolve away, and a player choosing SMAA where the camera asked for FXAA is
//! asking for a different filter rather than for less of one. It is still one
//! layer — [`EffectRequest::resolve`] applies it where the video clamp sits,
//! before the override and before the device.
//!
//! # An effect that is off is a frame with fewer passes, never a shader branch
//!
//! Each toggle removes work rather than selecting an alternative:
//!
//! * [`RenderEffects::SHADOWS`] off records no cull and no draw into the shadow
//!   atlas, and the atlas keeps the reversed-Z clear the pass writes — which
//!   every comparison against it reads as "fully lit".
//! * [`RenderEffects::AMBIENT_OCCLUSION`] off records neither occlusion pass and
//!   leaves the renderer's 1×1 white `R8Unorm` bound, which `mesh.slang`
//!   multiplies its ambient term by.
//! * [`RenderEffects::REFLECTIONS`] off records neither reflection pass and
//!   tonemaps the forward pass's own scene colour, which is bit-identical to the
//!   frame before that pair existed.
//! * [`RenderEffects::BLOOM`] off records no chain pass and takes no chain image
//!   out of the transient pool, and the tonemap reads whichever image it would
//!   have read before — bit-identical, on the reflection pair's terms exactly.
//!   It is off in [`RenderEffects::DEFAULT_STACK`], so that is the frame every
//!   view in this workspace draws until one asks otherwise.
//!
//! Topic 18 specifies the first two mechanisms in writing and refuses the
//! alternatives; the third is that document's own words about the march.
//!
//! # What sets each layer today
//!
//! | Layer | Source | Wired |
//! | --- | --- | --- |
//! | [`EffectRequest::camera`] | the view's own render stack | yes — a renderer per view, each with its own request |
//! | [`EffectRequest::video`] | `[engine.video]` | yes — `crcbl`'s start-up reads it: `GpuContextDesc::settings`, `GpuContext::effect_request` |
//! | [`EffectRequest::antialiasing`] | `[engine.video] antialiasing` | yes — the same read: `crcbl::settings::antialiasing`, `GpuContext::effect_request` |
//! | [`EffectRequest::programmatic`] | game code | yes — [`ForwardRenderer::set_effect_request`] |
//! | the device clamp | [`crcbl_hal::DeviceCaps`] | yes, and it removes nothing — see [`ForwardRenderer::device_effects`] |
//!
//! The camera row's source is **not** the render-stack RON topic 18 describes —
//! nothing in this workspace reads or writes RON — it is a
//! [`ForwardRenderer`](crate::ForwardRenderer) per view, each holding the
//! request its own view asked for. That is enough to make the layer real,
//! because what the layer means is "two views in one frame resolve to different
//! effect sets", and a RON file would only be a second way to write the same
//! field. `apps/lantern`'s in-scene monitor is the consumer: its
//! render-to-texture camera draws the room without reflections while the frame
//! it hangs in draws them, from one device, in one graph, through this one
//! function.
//!
//! Every row has a source now. The `[engine.video]` one arrives through the
//! umbrella rather than through this crate, because it has to: the keys are a
//! settings question and `crcbl-render` has no storage, so `crcbl::settings`
//! owns the one table mapping a key to a [`RenderEffects`] bit and
//! `crcbl::engine::GpuContext` does the read while it opens. A context is what
//! every sample and every `crcbl new` scaffold already opens before it builds a
//! renderer, which is what makes the layer free rather than opt-in, and the
//! read is infallible — a player with no settings file, and a platform that
//! names no settings directory, both mean "the player has not asked for less".
//!
//! **What a context reads is the player's answer, not a request.**
//! `effect_request` returns this struct with
//! [`video`](EffectRequest::video) and
//! [`antialiasing`](EffectRequest::antialiasing) filled in and the other two at
//! their defaults, so a caller with a per-view stack writes
//! `EffectRequest { camera, ..ctx.effect_request() }` and keeps both.
//!
//! [`ForwardRenderer::set_effect_request`]: crate::ForwardRenderer::set_effect_request
//! [`ForwardRenderer::device_effects`]: crate::ForwardRenderer::device_effects

bitflags::bitflags! {
    /// The effects `docs/plan/18-render-features.md` ships, one bit each.
    ///
    /// One bit per effect and **not** one per pass: the occlusion pass and its
    /// blur are one switch because the blur is not optional (`crate::ssao` says
    /// why), and the reflection march and its blur are one switch because the
    /// blur is the composite.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct RenderEffects: u32 {
        /// The shadow atlas: the sun's cascades and every shadowed light's
        /// tiles, their culls, and the depth-only pass that fills them.
        const SHADOWS = 1 << 0;
        /// Screen-space ambient occlusion — the occlusion pass and its
        /// depth-weighted blur, whose result scales the ambient term.
        const AMBIENT_OCCLUSION = 1 << 1;
        /// Screen-space reflections — the march and the blur that composites it
        /// into the scene colour.
        const REFLECTIONS = 1 << 2;
        /// The bloom chain — the threshold-free downsample pyramid, the tent
        /// upsample that walks it back down, and the composite that adds it to
        /// the scene colour.
        ///
        /// One bit for a variable number of passes, because the chain's length
        /// is a function of the target's extent rather than a choice — see
        /// `crate::bloom`. It is also the first bit here that is **not** in
        /// [`RenderEffects::DEFAULT_STACK`]; that constant says why.
        const BLOOM = 1 << 3;
        /// FXAA — the fullscreen edge filter that resolves the tonemapped
        /// frame into the target. [`Antialiasing::Fxaa`] is the rung a settings
        /// file names it by.
        ///
        /// **In [`RenderEffects::DEFAULT_STACK`]**, unlike [`BLOOM`](Self::BLOOM):
        /// this one is not a lens, it is a resolve, and it is the tier the
        /// resolve slot carries by default — [`Antialiasing::from_effects`] of
        /// the default stack answers [`Antialiasing::Fxaa`]. The higher tier,
        /// [`SMAA`](Self::SMAA), is the one kept out, and its doc says why.
        /// `docs/plan/49-antialiasing.md` is where the ladder is written down.
        const ANTIALIASING = 1 << 4;
        /// Volumetric fog — the froxel scatter, the column scan that turns it
        /// into a prefix, and the fullscreen composite over the frame.
        ///
        /// `docs/plan/51-volumetrics.md`'s ladder. **The second bit not in
        /// [`RenderEffects::DEFAULT_STACK`], and for a reason neither of the
        /// other two has**: this one does not add a term, it *moves* one. The
        /// medium it integrates is the same exponential height fog `mesh.slang`
        /// composites analytically, so a frame that ran both would charge the
        /// air twice — [`ForwardRenderer::add_passes`] zeroes the frame block's
        /// density when this is on, and the froxel path owns the medium
        /// instead.
        ///
        /// A view with [`Fog::NONE`](crate::Fog::NONE) draws the same frame
        /// either way, exactly: a zero density gives every froxel a
        /// transmittance of one and no radiance, and the composite multiplies
        /// by that one and adds those zeroes.
        ///
        /// [`ForwardRenderer::add_passes`]: crate::forward::ForwardRenderer::add_passes
        const VOLUMETRIC_FOG = 1 << 5;
        /// Auto-exposure — the luminance histogram of the finished frame, the
        /// reduce that turns it into one number, and the tonemap reading that
        /// number instead of the one a caller set.
        ///
        /// `docs/plan/43-render-standards.md` §6's rung, and **the third bit
        /// not in [`RenderEffects::DEFAULT_STACK`]**. The reason is
        /// [`VOLUMETRIC_FOG`](Self::VOLUMETRIC_FOG)'s rather than
        /// [`BLOOM`](Self::BLOOM)'s: it does not add a term to the picture, it
        /// takes the exposure away from the caller and computes it from the
        /// frame. A view that set an exposure and then found the engine
        /// overriding it would have been given a control that does nothing, so
        /// handing the control over is something a view asks for.
        ///
        /// It also has no additive-zero form to land behind — an exposure
        /// measured from the frame is not the exposure a caller happened to
        /// set — so switching it on moves every frame it is on for, which is the
        /// re-bless [`ANTIALIASING`](Self::ANTIALIASING) is held out by.
        ///
        /// There is **no time constant** on it yet: the exposure a frame is
        /// drawn with is measured from that frame, so a cut between two
        /// differently-lit shots lands in one frame rather than over a few
        /// tenths of a second. `docs/plan/48-post-processing.md` carries the
        /// adaptation as the next rung.
        const AUTO_EXPOSURE = 1 << 6;
        /// SMAA 1x — the edge detection, the blend-weight pass that reads the
        /// two committed lookup tables, and the neighbourhood blend.
        /// [`Antialiasing::Smaa`] is the rung a settings file names it by.
        ///
        /// `docs/plan/49-antialiasing.md`'s antialiasing ladder, second rung and
        /// **the higher of the two antialiasing tiers**. When it is set it takes
        /// the resolve slot instead of [`ANTIALIASING`](Self::ANTIALIASING):
        /// three passes where FXAA is one, over-blurring far less of the thin
        /// geometry and text that is FXAA's known weakness.
        ///
        /// **The two are never both run**, and that is what "a tier that is off
        /// is a frame with fewer passes" means here — the FXAA bit may stay set
        /// beside this one and is simply not recorded, because there is one
        /// resolve slot and this fills it.
        /// [`ForwardRenderer::add_passes`] is where the choice is made, once,
        /// and `crate::smaa` is the pass group.
        ///
        /// **Not in [`RenderEffects::DEFAULT_STACK`]**, on
        /// [`ANTIALIASING`](Self::ANTIALIASING)'s original terms: it is a
        /// resolve rather than a lens and belongs there on the merits, and what
        /// keeps it out is that swapping the resolve moves every golden in the
        /// suite at once. That flip is its own change with its own re-bless, and
        /// which tier the default stack should carry is a decision to take once
        /// goldens exist for both — `docs/backlog.md` holds the question.
        ///
        /// The bit is numbered after the tiers it postdates rather than beside
        /// the one it replaces, so no existing flag's value moves.
        ///
        /// [`ForwardRenderer::add_passes`]: crate::forward::ForwardRenderer::add_passes
        const SMAA = 1 << 7;
        /// Screen-space contact shadows — the short march along the sun's
        /// direction through the depth prepass, whose `R8Unorm` channel scales
        /// the sun's shadow term.
        ///
        /// `docs/plan/45-shadows.md`'s 2026-08-30 decision, and the rung the
        /// shadow ladder reached after the rotated disc: the sliver where a foot
        /// meets the floor or a book meets a shelf is finer than any atlas
        /// texel, so no bias and no filter can close it — a bias large enough to
        /// stop acne is large enough to detach the contact. `crate::shadow` is
        /// what the atlas answers for and `crate::contact_shadows` is the pass.
        ///
        /// **A screen-space term, so it leaves no trace on a tile and stacks
        /// with every rung above it**: it neither reads the atlas nor writes it,
        /// and a frame that switches it off records one fewer full-screen pass
        /// and binds the renderer's 1×1 white in its place.
        ///
        /// **Not in [`RenderEffects::DEFAULT_STACK`], and parked there rather
        /// than argued out of it.** The decision puts it *in* the default stack,
        /// with the low quality preset clearing it; what holds it out today is
        /// that switching it on moves every golden the workspace has. That flip
        /// and its re-bless is its own change — see the constant, which lists it
        /// with the three bits that are out on the merits.
        ///
        /// The bit is numbered after the tiers it postdates rather than beside
        /// the shadow bit it completes, so no existing flag's value moves —
        /// [`SMAA`](Self::SMAA)'s rule.
        const CONTACT_SHADOWS = 1 << 8;
    }
}

impl RenderEffects {
    /// What a view that has declared no render stack asks for.
    ///
    /// The three light-transport effects, and neither of the two post passes.
    /// The line is not arbitrary: shadows, ambient occlusion and reflections
    /// each approximate light transport that is **physically present in the
    /// scene**, so a view that says nothing about them is a view asking for the
    /// most correct picture the device can draw. Bloom is a property of a
    /// **lens** — topic 18 files it under the post stack, whose contents that
    /// document says are "data-driven per camera (RON: which passes,
    /// parameters)" — and a camera that has been given no stack has been given
    /// no lens.
    ///
    /// [`VOLUMETRIC_FOG`](Self::VOLUMETRIC_FOG) is out for a second reason: it
    /// does not add a term to the picture, it takes one away from the fragment
    /// stage and computes it somewhere else, and which of the two a view wants
    /// is a cost question rather than a correctness one.
    /// [`AUTO_EXPOSURE`](Self::AUTO_EXPOSURE) is out for a third, written on
    /// the bit: it takes a control away from the caller, and a view that set an
    /// exposure has said what it wants done with it. [`SMAA`](Self::SMAA) is
    /// out for a fourth, also written on the bit: the resolve slot is filled by
    /// [`ANTIALIASING`](Self::ANTIALIASING), which *is* in here, and moving it
    /// to the higher tier moves every golden in the suite — a decision to take
    /// once both tiers have goldens rather than one to inherit.
    /// [`CONTACT_SHADOWS`](Self::CONTACT_SHADOWS) is out for a fifth reason
    /// that is not a reason at all: `docs/plan/45-shadows.md` decided it belongs
    /// *here*, with the low preset clearing it, and it is parked outside only
    /// until the re-bless its first frame forces is taken on its own. It is the
    /// one member of this list that is expected to leave it.
    ///
    /// So the default is the most correct picture, no lens and the cheap
    /// antialiasing tier, and a view that wants more asks for it: through its
    /// own stack, or through [`EffectOverride`], which is the layer that may
    /// move a decision upward.
    ///
    /// This is what [`EffectRequest::default`]'s
    /// [`camera`](EffectRequest::camera) holds, and it is why adding the bloom
    /// bit — and then the antialiasing bit — did not change a single frame the
    /// engine already drew.
    pub const DEFAULT_STACK: Self = Self::all().difference(
        Self::BLOOM
            .union(Self::VOLUMETRIC_FOG)
            .union(Self::AUTO_EXPOSURE)
            .union(Self::SMAA)
            // **Parked, not decided.** `docs/plan/45-shadows.md` puts
            // [`CONTACT_SHADOWS`](Self::CONTACT_SHADOWS) *in* this stack and has
            // the low preset clear it; it sits in this list because switching it
            // on moves every golden in the workspace at once, and that re-bless
            // is a change of its own rather than a rider on the change that
            // built the pass. The other four are out on the merits and say so
            // above; this one is out on timing and says so here.
            .union(Self::CONTACT_SHADOWS),
    );

    /// Every effect and the one word a report spells it with, in bit order.
    ///
    /// **The length is the flag count, not a number written here**, so a fifth
    /// bit added to [`RenderEffects`] and left out of this table is a `cargo
    /// build` error rather than a row that silently stops mentioning it. That
    /// was the failure this table replaced: a hand-written list in
    /// `apps/lantern` that named every effect but [`BLOOM`](Self::BLOOM), in a
    /// sample whose summary line claimed to say what its frames drew.
    ///
    /// The length cannot see a table that reaches it by naming one bit twice —
    /// `every_effect_is_named_exactly_once_and_the_row_prints_them` is what
    /// does.
    const NAMES: [(Self, &'static str); Self::all().bits().count_ones() as usize] = [
        (Self::SHADOWS, "shadows"),
        (Self::AMBIENT_OCCLUSION, "ao"),
        (Self::REFLECTIONS, "ssr"),
        (Self::BLOOM, "bloom"),
        (Self::ANTIALIASING, "aa"),
        (Self::VOLUMETRIC_FOG, "vfog"),
        (Self::AUTO_EXPOSURE, "autoexp"),
        (Self::SMAA, "smaa"),
        (Self::CONTACT_SHADOWS, "contact"),
    ];

    /// This set as a debug panel row and a headless summary line spell it:
    /// `shadows ao ssr`, with a switched-off effect dropped, and `none` where
    /// they all are.
    ///
    /// One spelling for every sample, because every sample reports this and a
    /// second copy of the table is where one of them comes to disagree about
    /// what a frame drew.
    #[must_use]
    pub fn row(self) -> String {
        let on: Vec<&str> = Self::NAMES
            .into_iter()
            .filter(|(effect, _)| self.contains(*effect))
            .map(|(_, name)| name)
            .collect();
        if on.is_empty() {
            "none".to_string()
        } else {
            on.join(" ")
        }
    }
}

/// Which tier fills the frame's one antialiasing slot.
///
/// `docs/plan/49-antialiasing.md`'s eighth decision, taken 2026-08-30 with
/// Counter-Strike 2's video panel in front of it: the slot holds **one** filter,
/// so the settings seam holds one ladder rather than two independent bits a
/// panel could switch on together. The renderer still reads
/// [`RenderEffects::ANTIALIASING`] and [`RenderEffects::SMAA`] —
/// [`bits`](Self::bits) and [`from_effects`](Self::from_effects) are the join,
/// and [`ForwardRenderer::add_passes`] is where the bits become passes.
///
/// The rungs above this one — CMAA2 in SMAA's place, then MSAA 2×, 4× and 8× —
/// are that section's next two slices and are deliberately not here.
///
/// [`ForwardRenderer::add_passes`]: crate::forward::ForwardRenderer::add_passes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Antialiasing {
    /// Neither tier: the tonemap writes the caller's target and no resolve pass
    /// is recorded at all.
    None,
    /// FXAA 3.11 — [`RenderEffects::ANTIALIASING`], one fullscreen pass.
    Fxaa,
    /// SMAA 1x — [`RenderEffects::SMAA`], three passes and the higher tier.
    Smaa,
}

impl Antialiasing {
    /// The tiers, in the order a settings ladder climbs them.
    ///
    /// Ascending by cost and by what it removes, which is the order
    /// `apps/options`' `ANTIALIASING` row steps and the order
    /// [`from_name`](Self::from_name) searches.
    pub const ALL: [Self; 3] = [Self::None, Self::Fxaa, Self::Smaa];

    /// The two bits one resolve slot is made of.
    ///
    /// [`EffectRequest::resolve`] clears both before it sets the chosen one, so
    /// a set that reached it with both on cannot leave with both on.
    const SLOT: RenderEffects = RenderEffects::ANTIALIASING.union(RenderEffects::SMAA);

    /// The bits this tier sets, and no others.
    #[must_use]
    pub const fn bits(self) -> RenderEffects {
        match self {
            Self::None => RenderEffects::empty(),
            Self::Fxaa => RenderEffects::ANTIALIASING,
            Self::Smaa => RenderEffects::SMAA,
        }
    }

    /// The tier `effects` draws.
    ///
    /// **[`Smaa`](Self::Smaa) wins where both bits are set**, because that is
    /// the choice [`ForwardRenderer::add_passes`] makes when it fills the slot:
    /// this answers what the frame draws rather than what the set says.
    ///
    /// [`ForwardRenderer::add_passes`]: crate::forward::ForwardRenderer::add_passes
    #[must_use]
    pub const fn from_effects(effects: RenderEffects) -> Self {
        if effects.contains(RenderEffects::SMAA) {
            Self::Smaa
        } else if effects.contains(RenderEffects::ANTIALIASING) {
            Self::Fxaa
        } else {
            Self::None
        }
    }

    /// The snake_case word a settings file spells this tier with.
    ///
    /// **The one place a rung's spelling is written**, which is why
    /// [`from_name`](Self::from_name) searches [`ALL`](Self::ALL) through this
    /// rather than matching the strings a second time: two spellings of one
    /// rung is a row a screen saves and a start-up never reads back.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fxaa => "fxaa",
            Self::Smaa => "smaa",
        }
    }

    /// The tier `name` spells, or [`None`] for a word no rung
    /// wears.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.name() == name)
    }
}

/// A programmatic override: which effects are forced on, and which off.
///
/// The third layer, and the only one that may move a decision **either way** —
/// the two around it clamp downward. A game with its own quality logic drives
/// this directly, which is the escape hatch topic 39 names
/// `GpuContext::set_pacing` as the precedent for.
///
/// The two sets are kept disjoint by construction: [`force`](Self::force) is the
/// only way to write either, and it removes the bit from the other set. A
/// effect in both would make the resolution order depend on which of the two is
/// applied first, which is a rule nobody should have to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectOverride {
    on: RenderEffects,
    off: RenderEffects,
}

impl Default for EffectOverride {
    fn default() -> Self {
        Self::none()
    }
}

impl EffectOverride {
    /// Nothing overridden — the layers around this one decide.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            on: RenderEffects::empty(),
            off: RenderEffects::empty(),
        }
    }

    /// Forces every effect in `effects` on, off, or back to undecided.
    ///
    /// `Some(true)` is on, `Some(false)` is off, and [`None`] releases them —
    /// which is not the same as forcing them off, and is what a settings screen
    /// does when a player returns a row to "auto".
    #[must_use]
    pub const fn force(mut self, effects: RenderEffects, state: Option<bool>) -> Self {
        self.on = self.on.difference(effects);
        self.off = self.off.difference(effects);
        match state {
            Some(true) => self.on = self.on.union(effects),
            Some(false) => self.off = self.off.union(effects),
            None => {}
        }
        self
    }

    /// What this override says about `effect`, on [`force`](Self::force)'s
    /// terms.
    ///
    /// # Panics
    ///
    /// If `effect` is more than one flag: the answer would have to be one
    /// verdict about several independent switches.
    #[must_use]
    pub fn state(&self, effect: RenderEffects) -> Option<bool> {
        assert!(
            effect.bits().is_power_of_two(),
            "{effect:?} is not a single effect, so it has no one state"
        );
        if self.on.contains(effect) {
            Some(true)
        } else if self.off.contains(effect) {
            Some(false)
        } else {
            None
        }
    }

    /// Whether this override decides nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.on.is_empty() && self.off.is_empty()
    }
}

/// The layers a caller supplies, in the order they are applied.
///
/// A struct rather than positional arguments, for the reason this crate's
/// `SsrImages` is one: two of these fields are the same type, so positional
/// arguments could be swapped at a call site and the frame would still compile
/// and still draw a picture.
///
/// [`Default`] is [`RenderEffects::DEFAULT_STACK`] wanted by the view, no
/// quality clamp, no antialiasing choice and no override — which resolves to
/// whatever the device permits, and is what every frame the engine drew before
/// this type existed did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectRequest {
    /// What this **view** needs, from its render stack.
    ///
    /// Per camera because it is genuinely per view: topic 18's
    /// render-to-texture security monitor and its planar reflection do not want
    /// reflections of their own, and that is a property of the camera rather
    /// than of the player's hardware.
    ///
    /// **A renderer per view is what sets it**, rather than the render-stack RON
    /// topic 18 describes — see this module's table for why that is the same
    /// layer and not a substitute for it. `apps/lantern`'s monitor camera is the
    /// consumer in this tree.
    pub camera: RenderEffects,
    /// What the **player** allows, from `[engine.video]`.
    ///
    /// A quality setting, so it only ever clamps downward: a player who turned
    /// reflections off gets none, and a player who turned them on gets them
    /// where the view asked for them.
    ///
    /// **`crcbl`'s start-up is what sets it**: `crcbl::settings::VIDEO_KEYS` is
    /// the one place a key is spelled, `crcbl::engine::GpuContext` reads the
    /// namespace through `crcbl_store::settings::SettingsStack` while it opens,
    /// and `GpuContext::effect_request` is what a renderer built on that context
    /// is handed. See this module's table.
    ///
    /// [`RenderEffects::all`] is "the player has said nothing", which is what a
    /// missing key, a missing file and a missing settings directory all produce
    /// — a clamp that removes nothing rather than a frame with no effects in it.
    pub video: RenderEffects,
    /// Which antialiasing tier the **player** picked, from
    /// `[engine.video] antialiasing`.
    ///
    /// [`None`] is "the player has not picked one", and is what a
    /// missing key, a missing file and a missing settings directory all produce
    /// — the view's own stack keeps the slot it asked for.
    ///
    /// **The one layer that replaces rather than clamps**, and
    /// [`Antialiasing`]'s docs carry the argument: the slot holds one filter, so
    /// a player choosing SMAA where the camera asked for FXAA is asking for a
    /// different filter and not for less of one. It is applied after
    /// [`video`](Self::video) and before [`programmatic`](Self::programmatic),
    /// so game code still has the last word before the device — see
    /// [`resolve`](Self::resolve).
    pub antialiasing: Option<Antialiasing>,
    /// What this **moment** calls for, from game code.
    pub programmatic: EffectOverride,
}

impl Default for EffectRequest {
    /// [`RenderEffects::DEFAULT_STACK`] for the view, no quality clamp, no
    /// antialiasing choice and no override.
    ///
    /// **The camera field is the default stack rather than
    /// [`RenderEffects::all`]**, and that constant carries the argument: the
    /// effects that model the scene's own light transport are what a view asks
    /// for by saying nothing, and the lens effects are what it has to ask for.
    fn default() -> Self {
        Self {
            camera: RenderEffects::DEFAULT_STACK,
            video: RenderEffects::all(),
            antialiasing: None,
            programmatic: EffectOverride::none(),
        }
    }
}

impl EffectRequest {
    /// Applies the whole order and returns what the frame draws.
    ///
    /// ```text
    /// camera → clamped by video → the AA slot replaced by the player's tier
    ///        → moved either way by the override
    ///        → clamped by device, last and absolutely
    /// ```
    ///
    /// **The antialiasing tier sits inside the order rather than beside it**,
    /// and both of its neighbours matter. It is after the video clamp because a
    /// clamp can only take the resolve away, and the player picking a *different*
    /// filter is not a smaller ask; it is before the programmatic override
    /// because game code is the layer that may move a decision either way, and a
    /// tier applied after it would silently undo an override that had just
    /// forced a resolve on or off.
    ///
    /// `device` is what the hardware can run. **It cannot be overridden
    /// upward**: asking for an effect a device has no way to draw is what
    /// [`DeviceDesc::required_features`](crcbl_hal::DeviceDesc::required_features)
    /// is for, and it is not something a toggle can force.
    #[must_use]
    pub fn resolve(&self, device: RenderEffects) -> RenderEffects {
        let clamped = self.camera.intersection(self.video);
        let chosen = match self.antialiasing {
            Some(tier) => clamped.difference(Antialiasing::SLOT).union(tier.bits()),
            None => clamped,
        };
        chosen
            .union(self.programmatic.on)
            .difference(self.programmatic.off)
            .intersection(device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The four layers apply in the order topic 39 writes them down**, and
    /// each arm is the one that fails if two are swapped.
    ///
    /// Written as a table because the order is the whole point: an override
    /// applied before `[engine.video]` would be silently clamped away by a
    /// quality setting it is meant to escape, and a device clamp applied before
    /// the override would let a toggle force on an effect the hardware cannot
    /// draw. Both of those compile and both draw a plausible frame.
    #[test]
    fn the_layers_resolve_in_the_order_topic_39_specifies() {
        let all = RenderEffects::all();
        let shadows = RenderEffects::SHADOWS;
        let ao = RenderEffects::AMBIENT_OCCLUSION;

        // The default is `DEFAULT_STACK`: every effect that models the scene's
        // own light transport, which is the frame the engine drew before there
        // were toggles and before there was a lens effect to leave out of it.
        assert_eq!(
            EffectRequest::default().resolve(all),
            RenderEffects::DEFAULT_STACK
        );

        // 1. The camera's stack is the top: a view that does not ask for an
        //    effect does not get it, whatever the layers below say.
        let view = EffectRequest {
            camera: all.difference(RenderEffects::REFLECTIONS),
            ..EffectRequest::default()
        };
        assert_eq!(
            view.resolve(all),
            all.difference(RenderEffects::REFLECTIONS)
        );

        // 2. `[engine.video]` clamps downward and never upward. A player who
        //    turned an effect *on* that the view never asked for does not get
        //    it — which is the arm that fails if the two are unioned.
        let quality = EffectRequest {
            camera: shadows,
            video: all,
            ..EffectRequest::default()
        };
        assert_eq!(quality.resolve(all), shadows, "video must not add");
        let clamped = EffectRequest {
            camera: all,
            video: shadows,
            ..EffectRequest::default()
        };
        assert_eq!(clamped.resolve(all), shadows, "video must clamp");

        // 3. The override moves it either way, and it is applied *after* the
        //    quality clamp — so it can restore what the clamp took.
        let forced_on = EffectRequest {
            camera: all,
            video: shadows,
            programmatic: EffectOverride::none().force(ao, Some(true)),
            ..EffectRequest::default()
        };
        assert_eq!(
            forced_on.resolve(all),
            shadows.union(ao),
            "the override must escape the quality clamp, which is what it is for"
        );
        let forced_off = EffectRequest {
            programmatic: EffectOverride::none().force(shadows, Some(false)),
            ..EffectRequest::default()
        };
        assert_eq!(
            forced_off.resolve(all),
            RenderEffects::DEFAULT_STACK.difference(shadows)
        );

        // 4. The device clamps last and absolutely: an override forcing an
        //    effect on cannot conjure one the device has no way to draw.
        assert_eq!(
            forced_on.resolve(all.difference(ao)),
            shadows,
            "the device clamp must be last, and must not be overridable upward"
        );

        // 5. The antialiasing tier *replaces* the resolve slot rather than
        //    clamping it, which is the arm that fails if it is intersected with
        //    the camera's set the way `video` is: the camera here asks for FXAA
        //    and the player asked for SMAA, and an intersection leaves neither.
        let smaa = RenderEffects::SMAA;
        let picked = EffectRequest {
            camera: RenderEffects::DEFAULT_STACK,
            antialiasing: Some(Antialiasing::Smaa),
            ..EffectRequest::default()
        };
        assert_eq!(
            picked.resolve(all),
            RenderEffects::DEFAULT_STACK
                .difference(RenderEffects::ANTIALIASING)
                .union(smaa),
            "the player's tier must take the slot, not intersect with it"
        );
        assert_eq!(
            EffectRequest {
                antialiasing: Some(Antialiasing::None),
                ..EffectRequest::default()
            }
            .resolve(all),
            RenderEffects::DEFAULT_STACK.difference(Antialiasing::SLOT),
            "a player who chose no tier must empty the slot, both bits of it"
        );

        // 6. And it is applied *before* the override, which is the arm that
        //    fails if the two are swapped: game code forcing the resolve off is
        //    the last word, and a tier applied after it would put SMAA back.
        let forced_off_after = EffectRequest {
            antialiasing: Some(Antialiasing::Smaa),
            programmatic: EffectOverride::none().force(Antialiasing::SLOT, Some(false)),
            ..EffectRequest::default()
        };
        assert_eq!(
            forced_off_after.resolve(all),
            RenderEffects::DEFAULT_STACK.difference(Antialiasing::SLOT),
            "the override must run after the tier, not before it"
        );
        let forced_on_after = EffectRequest {
            antialiasing: Some(Antialiasing::None),
            programmatic: EffectOverride::none().force(RenderEffects::ANTIALIASING, Some(true)),
            ..EffectRequest::default()
        };
        assert_eq!(
            forced_on_after.resolve(all),
            RenderEffects::DEFAULT_STACK,
            "the override must be able to restore a slot the player emptied"
        );

        // 7. The device still clamps the tier, last and absolutely.
        assert_eq!(
            picked.resolve(all.difference(smaa)),
            RenderEffects::DEFAULT_STACK.difference(RenderEffects::ANTIALIASING),
            "a device with no SMAA must not draw the tier the player picked"
        );
    }

    /// **A tier is one word, one pair of bits, and the same tier back again.**
    ///
    /// The spellings are written out rather than looped off
    /// [`Antialiasing::ALL`], for the reason `crcbl::settings`' key table is:
    /// a table used as its own oracle cannot fail, and these words are the
    /// file-format promise — renaming one is a settings file every existing
    /// player has already written. The match is exhaustive, so a rung added to
    /// the enum arrives here as a build error rather than as a silent gap.
    #[test]
    fn a_tier_spells_one_word_and_sets_one_pair_of_bits() {
        for tier in Antialiasing::ALL {
            let (word, bits) = match tier {
                Antialiasing::None => ("none", RenderEffects::empty()),
                Antialiasing::Fxaa => ("fxaa", RenderEffects::ANTIALIASING),
                Antialiasing::Smaa => ("smaa", RenderEffects::SMAA),
            };
            assert_eq!(tier.name(), word);
            assert_eq!(Antialiasing::from_name(word), Some(tier));
            assert_eq!(tier.bits(), bits);
            assert!(
                Antialiasing::SLOT.contains(tier.bits()),
                "{tier:?} sets a bit outside the resolve slot"
            );
            assert_eq!(
                Antialiasing::from_effects(tier.bits()),
                tier,
                "{tier:?} did not survive the trip through its own bits"
            );
        }

        assert_eq!(
            Antialiasing::from_name("FXAA"),
            None,
            "the key is snake_case"
        );
        assert_eq!(
            Antialiasing::from_name("cmaa2"),
            None,
            "a rung that is not built"
        );
        assert_eq!(Antialiasing::from_name(""), None);

        // Every set the two bits can be in names a tier that is on the ladder,
        // which is what makes `ALL` the whole enum rather than most of it.
        for extra in [
            RenderEffects::empty(),
            RenderEffects::ANTIALIASING,
            RenderEffects::SMAA,
            Antialiasing::SLOT,
        ] {
            let tier = Antialiasing::from_effects(extra);
            assert!(
                Antialiasing::ALL.contains(&tier),
                "{extra:?} resolved to {tier:?}, which is not on the ladder"
            );
        }
        assert_eq!(
            Antialiasing::from_effects(Antialiasing::SLOT),
            Antialiasing::Smaa,
            "the higher tier fills the slot when both bits are set, as add_passes does",
        );

        // The rung a view that declared no stack draws, which is what the
        // `ANTIALIASING` row is born on.
        assert_eq!(
            Antialiasing::from_effects(RenderEffects::DEFAULT_STACK),
            Antialiasing::Fxaa,
        );
    }

    /// **The default stack is every effect but the two a view has to ask for,
    /// and a view that wants them can still get them.**
    ///
    /// Two claims, and the second is what keeps the first from being a way of
    /// switching bloom off for good. The camera layer is a *request*, not a
    /// clamp, so a view that declares its own stack — or a caller reaching for
    /// [`EffectOverride`], the one layer that may move a decision upward — puts
    /// the chain back in the frame. `crcbl::screenshot`'s bloom fixture is the
    /// consumer that does exactly that.
    ///
    /// What it guards is the claim [`RenderEffects::DEFAULT_STACK`]'s docs make:
    /// the lens, the froxel volume, auto-exposure, the higher antialiasing tier
    /// and — until its re-bless is taken — the contact march are what a view has
    /// to ask for, and they are the **only** ones. A
    /// default that quietly included one would have re-blessed every golden
    /// image in the tree, and each of them would still have been a plausible
    /// picture — which is the whole difficulty: a wrongly-defaulted post pass
    /// does not look like a bug.
    ///
    /// The antialiasing resolve was held out on the same terms for exactly one
    /// change, which is what re-blessed the suite; it is in the default now, and
    /// this asserts that none of the other four followed it in.
    /// [`RenderEffects::CONTACT_SHADOWS`] is the one on that path today, and
    /// this is what will go red on the change that moves it.
    #[test]
    fn the_default_stack_is_the_light_transport_effects_and_a_view_can_still_ask() {
        assert_eq!(
            RenderEffects::DEFAULT_STACK,
            RenderEffects::all().difference(
                RenderEffects::BLOOM
                    .union(RenderEffects::VOLUMETRIC_FOG)
                    .union(RenderEffects::AUTO_EXPOSURE)
                    .union(RenderEffects::SMAA)
                    .union(RenderEffects::CONTACT_SHADOWS)
            ),
        );
        let post = RenderEffects::BLOOM
            .union(RenderEffects::VOLUMETRIC_FOG)
            .union(RenderEffects::AUTO_EXPOSURE)
            .union(RenderEffects::SMAA)
            .union(RenderEffects::CONTACT_SHADOWS);
        assert!(!EffectRequest::default().camera.contains(post));
        assert!(
            !EffectRequest::default()
                .resolve(RenderEffects::all())
                .contains(post),
            "a view that said nothing must not get a post pass"
        );

        // The camera's own stack, which is the layer topic 18 puts the post
        // stack in.
        let declared = EffectRequest {
            camera: RenderEffects::all(),
            ..EffectRequest::default()
        };
        assert_eq!(declared.resolve(RenderEffects::all()), RenderEffects::all());

        // And the override, for a caller with no stack of its own.
        let forced = EffectRequest {
            programmatic: EffectOverride::none()
                .force(RenderEffects::BLOOM, Some(true))
                .force(RenderEffects::ANTIALIASING, Some(true))
                .force(RenderEffects::VOLUMETRIC_FOG, Some(true))
                .force(RenderEffects::AUTO_EXPOSURE, Some(true))
                .force(RenderEffects::SMAA, Some(true))
                .force(RenderEffects::CONTACT_SHADOWS, Some(true)),
            ..EffectRequest::default()
        };
        assert_eq!(forced.resolve(RenderEffects::all()), RenderEffects::all());
    }

    /// **An override's two halves cannot disagree**, because the setter is the
    /// only way to write either and it clears the other side.
    ///
    /// The failure this prevents is an override holding an effect in both sets,
    /// where the answer depends on whether the union or the difference is
    /// applied first — a rule that reads as an implementation detail and decides
    /// what a frame draws.
    #[test]
    fn forcing_an_effect_one_way_releases_the_other() {
        let ssr = RenderEffects::REFLECTIONS;
        let over = EffectOverride::none().force(ssr, Some(true));
        assert_eq!(over.state(ssr), Some(true));

        let over = over.force(ssr, Some(false));
        assert_eq!(over.state(ssr), Some(false));
        assert_eq!(
            EffectRequest {
                programmatic: over,
                ..EffectRequest::default()
            }
            .resolve(RenderEffects::all()),
            RenderEffects::DEFAULT_STACK.difference(ssr),
            "the second force must stand alone, not be unioned with the first"
        );

        let released = over.force(ssr, None);
        assert_eq!(released.state(ssr), None);
        assert!(released.is_empty());
    }

    /// **Every effect bit has a word, and [`RenderEffects::row`] prints every
    /// one of them.**
    ///
    /// The guard for the shape this table replaced. `RenderEffects::NAMES` is
    /// as long as the type has bits, so a new effect left unnamed does not
    /// compile — what that length cannot see is a table that reaches it by
    /// naming one bit twice, which is what the first half here asserts. The
    /// count is taken from `bitflags`' own declared list rather than from a
    /// number written down, so it moves when the flags do.
    ///
    /// The second half is the part that matters to a reader of a summary line:
    /// a name in the table that `row` never emits is an effect that is on and
    /// unmentioned, which reads exactly like an effect that is off.
    #[test]
    fn every_effect_is_named_exactly_once_and_the_row_prints_them() {
        assert_eq!(
            RenderEffects::NAMES.len(),
            RenderEffects::all().iter_names().count(),
            "the table has to name every flag `bitflags` declares, once each",
        );

        let mut named = RenderEffects::empty();
        for (effect, word) in RenderEffects::NAMES {
            assert!(
                effect.bits().is_power_of_two(),
                "{word} names {effect:?}, which is not a single effect",
            );
            assert!(!named.intersects(effect), "{effect:?} is named twice");
            named = named.union(effect);
        }
        assert_eq!(named, RenderEffects::all());

        let row = RenderEffects::all().row();
        let words: Vec<&str> = row.split(' ').collect();
        assert_eq!(words.len(), RenderEffects::NAMES.len(), "{row:?}");
        for (effect, word) in RenderEffects::NAMES {
            assert!(
                words.contains(&word),
                "{effect:?} is on and unmentioned in {row:?}"
            );
        }

        // The spelling every sample's summary line and debug panel already
        // print, which this must not have moved.
        assert_eq!(RenderEffects::DEFAULT_STACK.row(), "shadows ao ssr aa");
        assert_eq!(RenderEffects::empty().row(), "none");
        assert_eq!(RenderEffects::BLOOM.row(), "bloom");
    }
}
