//! Which of `docs/plan/18-render-features.md`'s effects a frame draws, and
//! `docs/plan/39-capabilities.md`'s four-layer order that decides it.
//!
//! ```text
//! camera stack (RON) declares what the view wants
//!   → [engine.video] clamps it downward as a player quality setting
//!   → programmatic override may set it either way
//!   → device capability clamps it downward, last and absolutely
//! ```
//!
//! [`EffectRequest`] carries the first three — they are what a caller supplies
//! — and [`EffectRequest::resolve`] applies the whole order in one place, which
//! is the property topic 39 asks for by name. The renderer holds one request and
//! one resolved set, and every pass it records reads the resolved set; nothing
//! else in this crate branches on a layer.
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
//! **What a context reads is a ceiling, not a request.** `effect_request`
//! returns this struct with [`video`](EffectRequest::video) filled in and the
//! other two at their defaults, so a caller with a per-view stack writes
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
    }
}

impl RenderEffects {
    /// What a view that has declared no render stack asks for.
    ///
    /// Every effect but [`BLOOM`](Self::BLOOM), and the line between them is not
    /// arbitrary: shadows, ambient occlusion and reflections each approximate
    /// light transport that is **physically present in the scene**, so a view
    /// that says nothing about them is a view asking for the most correct
    /// picture the device can draw. Bloom is a property of a **lens** — topic 18
    /// files it under the post stack, whose contents that document says are
    /// "data-driven per camera (RON: which passes, parameters)" — and a camera
    /// that has been given no stack has been given no lens.
    ///
    /// So the default is the most correct picture and no lens, and a view that
    /// wants the lens asks for it: through its own stack, or through
    /// [`EffectOverride`], which is the layer that may move a decision upward.
    ///
    /// This is what [`EffectRequest::default`]'s
    /// [`camera`](EffectRequest::camera) holds, and it is why adding the bloom
    /// bit did not change a single frame the engine already drew.
    pub const DEFAULT_STACK: Self = Self::all().difference(Self::BLOOM);

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

/// The three layers a caller supplies, in the order they are applied.
///
/// A struct rather than three arguments, for the reason this crate's `SsrImages`
/// is one: two of these fields are the same type, so positional arguments could
/// be swapped at a call site and the frame would still compile and still draw a
/// picture.
///
/// [`Default`] is [`RenderEffects::DEFAULT_STACK`] wanted by the view, no
/// quality clamp and no override — which resolves to whatever the device
/// permits, and is what every frame the engine drew before this type existed
/// did.
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
    /// What this **moment** calls for, from game code.
    pub programmatic: EffectOverride,
}

impl Default for EffectRequest {
    /// [`RenderEffects::DEFAULT_STACK`] for the view, no quality clamp and no
    /// override.
    ///
    /// **The camera field is the default stack rather than
    /// [`RenderEffects::all`]**, and that constant carries the argument: the
    /// effects that model the scene's own light transport are what a view asks
    /// for by saying nothing, and the lens effects are what it has to ask for.
    fn default() -> Self {
        Self {
            camera: RenderEffects::DEFAULT_STACK,
            video: RenderEffects::all(),
            programmatic: EffectOverride::none(),
        }
    }
}

impl EffectRequest {
    /// Applies the whole order and returns what the frame draws.
    ///
    /// ```text
    /// camera → clamped by video → moved either way by the override
    ///        → clamped by device, last and absolutely
    /// ```
    ///
    /// `device` is what the hardware can run. **It cannot be overridden
    /// upward**: asking for an effect a device has no way to draw is what
    /// [`DeviceDesc::required_features`](crcbl_hal::DeviceDesc::required_features)
    /// is for, and it is not something a toggle can force.
    #[must_use]
    pub fn resolve(&self, device: RenderEffects) -> RenderEffects {
        self.camera
            .intersection(self.video)
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
    }

    /// **The default stack is every effect but the lens one, and a view that
    /// wants the lens can still get it.**
    ///
    /// Two claims, and the second is what keeps the first from being a way of
    /// switching bloom off for good. The camera layer is a *request*, not a
    /// clamp, so a view that declares its own stack — or a caller reaching for
    /// [`EffectOverride`], the one layer that may move a decision upward — puts
    /// the chain back in the frame. `crcbl::screenshot`'s bloom fixture is the
    /// consumer that does exactly that.
    ///
    /// What it guards is the claim [`RenderEffects::DEFAULT_STACK`]'s docs make:
    /// adding the bloom bit changed no frame this workspace already drew. A
    /// default that quietly included it would have re-blessed every golden image
    /// in the tree, and each of them would still have been a plausible picture.
    #[test]
    fn the_default_stack_is_every_effect_but_the_lens_and_a_view_can_still_ask() {
        assert_eq!(
            RenderEffects::DEFAULT_STACK,
            RenderEffects::all().difference(RenderEffects::BLOOM),
        );
        assert!(
            !EffectRequest::default()
                .camera
                .contains(RenderEffects::BLOOM)
        );
        assert!(
            !EffectRequest::default()
                .resolve(RenderEffects::all())
                .contains(RenderEffects::BLOOM),
            "a view that said nothing must not get a lens effect"
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
            programmatic: EffectOverride::none().force(RenderEffects::BLOOM, Some(true)),
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
        assert_eq!(RenderEffects::DEFAULT_STACK.row(), "shadows ao ssr");
        assert_eq!(RenderEffects::empty().row(), "none");
        assert_eq!(RenderEffects::BLOOM.row(), "bloom");
    }
}
