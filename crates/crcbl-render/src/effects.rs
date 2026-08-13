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
//!
//! Topic 18 specifies the first two mechanisms in writing and refuses the
//! alternatives; the third is that document's own words about the march.
//!
//! # What sets each layer today
//!
//! | Layer | Source | Wired |
//! | --- | --- | --- |
//! | [`EffectRequest::camera`] | a render-stack RON | **no** — there is no render-stack RON, and one camera draws a frame |
//! | [`EffectRequest::video`] | `[engine.video]` | **no** — `crcbl_store::settings` reads that namespace and nothing builds a stack at startup |
//! | [`EffectRequest::programmatic`] | game code | yes — [`ForwardRenderer::set_effect_request`] |
//! | the device clamp | [`crcbl_hal::DeviceCaps`] | yes, and it removes nothing — see [`ForwardRenderer::device_effects`] |
//!
//! The two unwired rows are fields nothing but a test writes, and they are here
//! rather than left out because the *order* is the thing being built: a
//! resolution point missing two of its four inputs cannot be shown to apply them
//! in the right order, and adding them later would move every caller.
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
/// [`Default`] is every effect wanted by the view, no quality clamp and no
/// override — which resolves to whatever the device permits, and is what every
/// frame the engine drew before this type existed did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectRequest {
    /// What this **view** needs, from its render stack.
    ///
    /// Per camera because it is genuinely per view: topic 18's
    /// render-to-texture security monitor and its planar reflection do not want
    /// reflections of their own, and that is a property of the camera rather
    /// than of the player's hardware.
    ///
    /// **Nothing sets this today** — there is no render-stack RON and one camera
    /// draws a frame. See this module's table.
    pub camera: RenderEffects,
    /// What the **player** allows, from `[engine.video]`.
    ///
    /// A quality setting, so it only ever clamps downward: a player who turned
    /// reflections off gets none, and a player who turned them on gets them
    /// where the view asked for them.
    ///
    /// **Nothing sets this today.** `crcbl_store::settings::SettingsStack` reads
    /// the `[engine.video]` namespace and no startup builds one. See this
    /// module's table.
    pub video: RenderEffects,
    /// What this **moment** calls for, from game code.
    pub programmatic: EffectOverride,
}

impl Default for EffectRequest {
    fn default() -> Self {
        Self {
            camera: RenderEffects::all(),
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

        // The default is every effect: the frame the engine drew before there
        // were toggles.
        assert_eq!(EffectRequest::default().resolve(all), all);

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
        assert_eq!(forced_off.resolve(all), all.difference(shadows));

        // 4. The device clamps last and absolutely: an override forcing an
        //    effect on cannot conjure one the device has no way to draw.
        assert_eq!(
            forced_on.resolve(all.difference(ao)),
            shadows,
            "the device clamp must be last, and must not be overridable upward"
        );
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
            RenderEffects::all().difference(ssr),
            "the second force must stand alone, not be unioned with the first"
        );

        let released = over.force(ssr, None);
        assert_eq!(released.state(ssr), None);
        assert!(released.is_empty());
    }
}
