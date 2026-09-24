//! The camera layer of [`crate::effects`]' four-layer order, as a file.
//!
//! [`RenderEffects`] is what a frame draws; [`CameraStack`] is what a **view
//! asks for**, written down. `docs/plan/18-render-features.md` says the post
//! stack is "data-driven per camera (RON: which passes, parameters)" and the
//! toggle layering `docs/notes/rendering.md` records puts that layer at the top
//! of the resolution order, above the player's `[engine.video]` clamp; this module is
//! the reader and the writer of that file, and [`CameraStack::compile`] is the
//! step that turns it into the bits [`EffectRequest::camera`] carries.
//!
//! ```
//! use crcbl_render::stack::CameraStack;
//! use crcbl_render::RenderEffects;
//!
//! let stack = CameraStack::from_ron(
//!     "(shadows: Some(()), ambient_occlusion: Some(()), bloom: Some(()))",
//! )?;
//! assert_eq!(
//!     stack.compile(),
//!     RenderEffects::SHADOWS
//!         .union(RenderEffects::AMBIENT_OCCLUSION)
//!         .union(RenderEffects::BLOOM),
//! );
//! # Ok::<(), crcbl_render::stack::StackError>(())
//! ```
//!
//! # A pass is present or absent, and that is the whole file
//!
//! Every field is an [`Option`] of a pass, and [`compile`](CameraStack::compile)
//! sets that pass's bit exactly where the option is [`Some`] — which is the same
//! statement `crate::effects` makes about the renderer: an effect that is off is
//! a frame with fewer passes, never a shader branch. A field a file leaves out
//! is [`None`], so the shortest legal stack is `()` and draws nothing.
//!
//! # The pass types are empty, and each says what it would carry
//!
//! Every pass here but [`AntialiasingPass`] is a field-less struct. That is not
//! a placeholder for parameters this module forgot: the renderer's per-pass
//! parameters are the setters a caller already has —
//! [`ForwardRenderer::set_fog`], [`set_exposure`], [`set_exposure_adaptation`],
//! [`set_tonemap_curve`] — and each of those takes a type that belongs to the
//! pass rather than to this file. Giving a pass a parameter here means giving
//! that type a serialized form and one owner, which is a slice of its own;
//! `docs/backlog.md` lists which passes are waiting for one.
//!
//! [`AntialiasingPass`] is the exception because the thing it names is not a
//! pass but a **slot**. [`Antialiasing`] holds the argument: there is one
//! resolve, [`RenderEffects::ANTIALIASING`] and [`RenderEffects::CMAA2`] are two
//! bits of one ladder rung rather than two independent switches, and
//! `EffectRequest::resolve` clears the whole slot before it fills it. So this
//! file has **one** antialiasing field naming a tier, and no per-tier field
//! beside it: two spellings of one rung is exactly what the antialiasing
//! ladder refused when it collapsed the two bits into one
//! (`docs/notes/rendering.md`).
//!
//! [`EffectRequest::camera`]: crate::EffectRequest::camera
//! [`ForwardRenderer::set_fog`]: crate::ForwardRenderer::set_fog
//! [`set_exposure`]: crate::ForwardRenderer::set_exposure
//! [`set_exposure_adaptation`]: crate::ForwardRenderer::set_exposure_adaptation
//! [`set_tonemap_curve`]: crate::ForwardRenderer::set_tonemap_curve

use serde::{Deserialize, Serialize};

use crate::effects::{Antialiasing, RenderEffects};

/// The shadow atlas — [`RenderEffects::SHADOWS`].
///
/// Field-less, on this module's terms: the atlas budget and
/// [`ForwardRenderer::set_shadow_cadence`](crate::ForwardRenderer::set_shadow_cadence)'s
/// cadence are the parameters it would carry, and neither has a serialized form
/// yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowsPass {}

/// Screen-space ambient occlusion — [`RenderEffects::AMBIENT_OCCLUSION`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientOcclusionPass {}

/// Screen-space reflections — [`RenderEffects::REFLECTIONS`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReflectionsPass {}

/// The bloom chain — [`RenderEffects::BLOOM`].
///
/// The chain's length is a function of the target's extent rather than a choice,
/// which is why the bit is one bit for a variable number of passes; the scalar
/// the composite adds it back with is the parameter this would carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BloomPass {}

/// The frame's one antialiasing resolve, and which tier fills it.
///
/// The one pass here with a field, and [`Antialiasing`] is why: the resolve slot
/// holds one filter, so a view names a rung rather than switching two bits.
/// [`CameraStack::compile`] puts [`Antialiasing::bits`] in and nothing else, so
/// a stack can no more ask for both tiers than a settings file can.
///
/// [`Antialiasing::None`] is a view asking for **no** resolve, which is not the
/// same as leaving the field out — both compile to no antialiasing bits today,
/// and they differ to a reader: one is a view that said so.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntialiasingPass {
    /// Which rung of the antialiasing ladder this view wants.
    pub tier: Antialiasing,
}

/// Volumetric fog — [`RenderEffects::VOLUMETRIC_FOG`].
///
/// The medium it integrates is the [`Fog`](crate::Fog) a view hands
/// [`ForwardRenderer::set_fog`](crate::ForwardRenderer::set_fog), which is a
/// scene property rather than this pass's own: a view with the bit off still
/// composites that fog analytically. So the parameter this would carry is the
/// froxel sample count, and there is no setter for one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumetricFogPass {}

/// Auto-exposure — [`RenderEffects::AUTO_EXPOSURE`].
///
/// [`ExposureAdaptation`](crate::ExposureAdaptation) is its parameter and it
/// arrives through
/// [`ForwardRenderer::set_exposure_adaptation`](crate::ForwardRenderer::set_exposure_adaptation),
/// because the rates are per-second and there is no clock in this crate to take
/// a frame delta from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoExposurePass {}

/// Screen-space contact shadows — [`RenderEffects::CONTACT_SHADOWS`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactShadowsPass {}

/// What one view asks its frame to draw, as a file.
///
/// One optional pass per effect [`RenderEffects`] carries, bar the two
/// antialiasing bits, which are one [`AntialiasingPass`] naming a tier — this
/// module's header says why. [`compile`](Self::compile) is the whole of the
/// mapping: a bit per [`Some`].
///
/// # Unknown fields are refused
///
/// `deny_unknown_fields`, so a stack naming a pass this engine does not have —
/// a typo, a rung from a later version, a tier's own word written as though it
/// were a field — fails to parse rather than silently drawing something else.
/// [`StackError`] names the line, the column and the field.
///
/// # Round trip
///
/// ```
/// use crcbl_render::stack::CameraStack;
///
/// let stack = CameraStack::default();
/// assert_eq!(CameraStack::from_ron(&stack.to_ron())?, stack);
/// # Ok::<(), crcbl_render::stack::StackError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraStack {
    /// The shadow atlas and everything that fills it.
    #[serde(default)]
    pub shadows: Option<ShadowsPass>,
    /// The occlusion pass and its depth-weighted blur.
    #[serde(default)]
    pub ambient_occlusion: Option<AmbientOcclusionPass>,
    /// The reflection march and the blur that composites it.
    #[serde(default)]
    pub reflections: Option<ReflectionsPass>,
    /// The bloom chain.
    #[serde(default)]
    pub bloom: Option<BloomPass>,
    /// The frame's one antialiasing resolve, and which tier fills it.
    #[serde(default)]
    pub antialiasing: Option<AntialiasingPass>,
    /// The froxel scatter, its column scan and the composite over the frame.
    #[serde(default)]
    pub volumetric_fog: Option<VolumetricFogPass>,
    /// The luminance histogram, its reduce, and the tonemap reading the result.
    #[serde(default)]
    pub auto_exposure: Option<AutoExposurePass>,
    /// The short march along the sun's direction through the depth prepass.
    #[serde(default)]
    pub contact_shadows: Option<ContactShadowsPass>,
}

impl Default for CameraStack {
    /// [`default_stack`](Self::default_stack) — the file form of
    /// [`RenderEffects::DEFAULT_STACK`].
    fn default() -> Self {
        Self::default_stack()
    }
}

impl CameraStack {
    /// A stack that asks for nothing: every field [`None`].
    ///
    /// What a file holding `()` parses to, and the base a stack is built up
    /// from — [`default_stack`](Self::default_stack) is written on it.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            shadows: None,
            ambient_occlusion: None,
            reflections: None,
            bloom: None,
            antialiasing: None,
            volumetric_fog: None,
            auto_exposure: None,
            contact_shadows: None,
        }
    }

    /// The file form of [`RenderEffects::DEFAULT_STACK`]: a pass exactly where
    /// that constant has a bit.
    ///
    /// The three light-transport effects and the resolve the default carries —
    /// a view that has been given no lens. `default_stack().compile() ==
    /// RenderEffects::DEFAULT_STACK` is asserted, so the two cannot drift: this
    /// is the same statement written twice, once as bits and once as a file, and
    /// the file is the one a demo can edit.
    #[must_use]
    pub const fn default_stack() -> Self {
        Self {
            shadows: Some(ShadowsPass {}),
            ambient_occlusion: Some(AmbientOcclusionPass {}),
            reflections: Some(ReflectionsPass {}),
            antialiasing: Some(AntialiasingPass {
                tier: Antialiasing::Cmaa2,
            }),
            ..Self::empty()
        }
    }

    /// The effect set this stack asks for: a bit per [`Some`].
    ///
    /// **This is what the camera layer of the resolution order carries.** It is
    /// a *request*, not what the frame draws — the player's `[engine.video]`
    /// clamp, the programmatic override and the device all still apply, in that
    /// order, and `EffectRequest::resolve` is where they do.
    ///
    /// The antialiasing field contributes [`Antialiasing::bits`] rather than one
    /// bit, which is how a stack names the higher tier without a second field.
    #[must_use]
    pub const fn compile(&self) -> RenderEffects {
        let mut effects = RenderEffects::empty();
        if self.shadows.is_some() {
            effects = effects.union(RenderEffects::SHADOWS);
        }
        if self.ambient_occlusion.is_some() {
            effects = effects.union(RenderEffects::AMBIENT_OCCLUSION);
        }
        if self.reflections.is_some() {
            effects = effects.union(RenderEffects::REFLECTIONS);
        }
        if self.bloom.is_some() {
            effects = effects.union(RenderEffects::BLOOM);
        }
        if let Some(pass) = self.antialiasing {
            effects = effects.union(pass.tier.bits());
        }
        if self.volumetric_fog.is_some() {
            effects = effects.union(RenderEffects::VOLUMETRIC_FOG);
        }
        if self.auto_exposure.is_some() {
            effects = effects.union(RenderEffects::AUTO_EXPOSURE);
        }
        if self.contact_shadows.is_some() {
            effects = effects.union(RenderEffects::CONTACT_SHADOWS);
        }
        effects
    }

    /// Parses one RON document.
    ///
    /// # Errors
    ///
    /// [`StackError`] if the text is not RON, or is RON that is not this struct:
    /// an unknown field, a tier that is not on the ladder, a missing bracket.
    /// The error names the line, the column and — where ron can say — the field.
    pub fn from_ron(text: &str) -> Result<Self, StackError> {
        ron::from_str(text).map_err(StackError::from_spanned)
    }

    /// Writes this stack back out, deterministically.
    ///
    /// **Byte-identical for equal stacks, on every platform.** Fields print in
    /// struct order because that is the order serde's derive visits them, and
    /// the newline is pinned to `\n` here rather than left to
    /// [`ron::ser::PrettyConfig`]'s default, which is `\r\n` on Windows — a
    /// writer whose output depended on the host is not one a scene format can
    /// keep in git. No field carries a number today, so what a float would print
    /// as is ron's answer and not this function's.
    ///
    /// # Panics
    ///
    /// Never, in practice: this struct is options of field-less structs and one
    /// C-like enum, and ron's serializer has no failing path over those. The
    /// `Result` exists for maps with non-string keys and for `Serialize`
    /// implementations that raise their own errors, and this type has neither.
    #[must_use]
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, Self::pretty())
            .expect("a CameraStack has no serializer path that can fail")
    }

    /// The one writer configuration, so [`to_ron`](Self::to_ron) and anything
    /// that later writes a stack cannot disagree about what a file looks like.
    fn pretty() -> ron::ser::PrettyConfig {
        ron::ser::PrettyConfig::new()
            .new_line("\n")
            .indentor("    ")
            .struct_names(true)
    }
}

/// Why a string is not a [`CameraStack`].
///
/// ron's own [`SpannedError`](ron::error::SpannedError) reduced to the three
/// things a person fixing the file needs — where, and what — so a caller
/// reporting it does not have to depend on ron's types. The message is ron's
/// verbatim, which is what carries the offending field's name.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("line {line}, column {column}: {message}")]
pub struct StackError {
    line: usize,
    column: usize,
    message: String,
}

impl StackError {
    /// Where the parser was when it gave up: the **start** of the span, which
    /// for an unknown field is the field's own name rather than the end of the
    /// document.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// The column of [`line`](Self::line), 1-based as ron counts it.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    /// What ron said, without the position it says it at.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    fn from_spanned(error: ron::error::SpannedError) -> Self {
        Self {
            line: error.span.start.line,
            column: error.span.start.col,
            message: error.code.to_string(),
        }
    }
}

// `Instance::create_device` is native-only: see the `crcbl_hal::device` module docs.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// **Every field survives the round trip, and the writer is deterministic.**
    ///
    /// Two claims in one, and the second is the one a scene format stands on: a
    /// stack written twice has to be the same bytes twice, or a file that
    /// nothing edited still shows up in a diff. The stack is built with every
    /// field `Some` rather than from `default_stack`, because a round trip that
    /// only visits the fields the default fills would pass with a field that
    /// serializes and cannot be read back.
    #[test]
    fn every_field_survives_the_round_trip_and_the_writer_repeats_itself() {
        let full = CameraStack {
            shadows: Some(ShadowsPass {}),
            ambient_occlusion: Some(AmbientOcclusionPass {}),
            reflections: Some(ReflectionsPass {}),
            bloom: Some(BloomPass {}),
            antialiasing: Some(AntialiasingPass {
                tier: Antialiasing::Cmaa2,
            }),
            volumetric_fog: Some(VolumetricFogPass {}),
            auto_exposure: Some(AutoExposurePass {}),
            contact_shadows: Some(ContactShadowsPass {}),
        };
        let text = full.to_ron();
        assert_eq!(
            CameraStack::from_ron(&text).expect("what to_ron wrote has to parse"),
            full,
            "{text}"
        );
        assert_eq!(text, full.to_ron(), "the writer is not deterministic");
        assert!(
            !text.contains('\r'),
            "the newline is pinned to \\n, so a Windows host writes the same bytes: {text:?}"
        );

        // And the empty end of the range, which is the file a view that asks for
        // nothing writes.
        let empty = CameraStack::empty();
        assert_eq!(
            CameraStack::from_ron(&empty.to_ron()).expect("an empty stack parses"),
            empty
        );
        assert_eq!(empty.compile(), RenderEffects::empty());
    }

    /// **A file that names a field this engine has no pass for is refused, and
    /// the refusal says where and which.**
    ///
    /// `deny_unknown_fields` is what refuses it. Without the attribute serde
    /// skips the value and parses the rest, so a stack whose `reflectons` was
    /// misspelled would draw without reflections and report success — which is
    /// a frame that is wrong and plausible, this crate's whole difficulty.
    ///
    /// `smaa` is the unknown field the test uses on purpose: it is the word the
    /// tier CMAA2 replaced was spelled with, and a file still holding it has to
    /// be told rather than quietly drawing something else. This file has one
    /// antialiasing slot instead, and the refusal is where a reader finds that
    /// out.
    #[test]
    fn an_unknown_field_is_refused_by_line_column_and_name() {
        let error = CameraStack::from_ron("(\n    shadows: Some(()),\n    smaa: Some(()),\n)")
            .expect_err("`smaa` is not a field of CameraStack");
        assert_eq!(error.line(), 3, "{error}");
        assert_eq!(error.column(), 5, "{error}");
        assert!(
            error.message().contains("smaa"),
            "the message has to name the field: {error}"
        );
        assert!(
            error.to_string().contains("line 3, column 5"),
            "the Display has to carry the position: {error}"
        );

        // A tier that is not on the ladder, and syntax that is not RON at all:
        // both come back as this error rather than as a stack.
        assert!(CameraStack::from_ron("(antialiasing: Some((tier: taa)))").is_err());
        assert!(CameraStack::from_ron("not ron").is_err());
    }

    /// **The default stack is `RenderEffects::DEFAULT_STACK`, written as a
    /// file.**
    ///
    /// The one assertion that keeps this module honest about what a view gets
    /// for saying nothing: `EffectRequest::default().camera` is that constant,
    /// so a `default_stack` that had drifted from it would hand a demo reading
    /// its own file a different frame from the demo beside it that reads none.
    #[test]
    fn the_default_stack_compiles_to_the_default_stack() {
        assert_eq!(
            CameraStack::default_stack().compile(),
            RenderEffects::DEFAULT_STACK
        );
        assert_eq!(CameraStack::default(), CameraStack::default_stack());
    }

    /// **Each field sets its own bit and no other, and between them they name
    /// every bit `RenderEffects` has.**
    ///
    /// The table is written out rather than looped off `compile`, because a
    /// table used as its own oracle cannot fail. The second half is the
    /// tripwire: a tenth effect bit added to `RenderEffects` and given no field
    /// here fails this, rather than becoming an effect no file can ask for.
    #[test]
    fn a_field_sets_its_own_bit_and_the_fields_cover_every_bit() {
        let one = |stack: CameraStack| stack.compile();
        for (stack, bit) in [
            (
                CameraStack {
                    shadows: Some(ShadowsPass {}),
                    ..CameraStack::empty()
                },
                RenderEffects::SHADOWS,
            ),
            (
                CameraStack {
                    ambient_occlusion: Some(AmbientOcclusionPass {}),
                    ..CameraStack::empty()
                },
                RenderEffects::AMBIENT_OCCLUSION,
            ),
            (
                CameraStack {
                    reflections: Some(ReflectionsPass {}),
                    ..CameraStack::empty()
                },
                RenderEffects::REFLECTIONS,
            ),
            (
                CameraStack {
                    bloom: Some(BloomPass {}),
                    ..CameraStack::empty()
                },
                RenderEffects::BLOOM,
            ),
            (
                CameraStack {
                    volumetric_fog: Some(VolumetricFogPass {}),
                    ..CameraStack::empty()
                },
                RenderEffects::VOLUMETRIC_FOG,
            ),
            (
                CameraStack {
                    auto_exposure: Some(AutoExposurePass {}),
                    ..CameraStack::empty()
                },
                RenderEffects::AUTO_EXPOSURE,
            ),
            (
                CameraStack {
                    contact_shadows: Some(ContactShadowsPass {}),
                    ..CameraStack::empty()
                },
                RenderEffects::CONTACT_SHADOWS,
            ),
        ] {
            assert_eq!(one(stack), bit, "{stack:?}");
        }

        // Every bit of the resolve slot, and nothing else in it.
        let mut covered = RenderEffects::empty();
        for tier in Antialiasing::ALL {
            let stack = CameraStack {
                antialiasing: Some(AntialiasingPass { tier }),
                ..CameraStack::empty()
            };
            assert_eq!(stack.compile(), tier.bits(), "{tier:?}");
            assert_eq!(
                Antialiasing::from_effects(stack.compile()),
                tier,
                "{tier:?} did not survive its own stack"
            );
            covered = covered.union(tier.bits());
        }

        // …and the fields between them reach every flag the type declares.
        let mut all = CameraStack {
            shadows: Some(ShadowsPass {}),
            ambient_occlusion: Some(AmbientOcclusionPass {}),
            reflections: Some(ReflectionsPass {}),
            bloom: Some(BloomPass {}),
            antialiasing: None,
            volumetric_fog: Some(VolumetricFogPass {}),
            auto_exposure: Some(AutoExposurePass {}),
            contact_shadows: Some(ContactShadowsPass {}),
        };
        covered = covered.union(all.compile());
        all.antialiasing = Some(AntialiasingPass {
            tier: Antialiasing::Fxaa,
        });
        covered = covered.union(all.compile());
        assert_eq!(
            covered,
            RenderEffects::all(),
            "an effect bit no field of CameraStack names",
        );
    }

    /// **A tier is spelled in a file the way `crcbl::settings` spells it in
    /// one**, and every rung round-trips through RON.
    ///
    /// The words are the file-format promise — a stack an author wrote is a
    /// file that has to go on parsing — and `Antialiasing::name` is where they
    /// are written down for the settings seam. Two spellings of one rung is the
    /// failure this asserts against: the serde attribute and that function are
    /// separate declarations of the same vocabulary.
    #[test]
    fn a_tier_is_spelled_the_way_the_settings_seam_spells_it() {
        for tier in Antialiasing::ALL {
            let stack = CameraStack {
                antialiasing: Some(AntialiasingPass { tier }),
                ..CameraStack::empty()
            };
            let text = stack.to_ron();
            assert!(
                text.contains(&format!("tier: {}", tier.name())),
                "{tier:?} is spelled {:?} by the settings seam and {text:?} by this writer",
                tier.name(),
            );
            assert_eq!(
                CameraStack::from_ron(&text).expect("a tier round-trips"),
                stack
            );
        }
    }

    /// **`set_camera_stack` moves the camera layer and leaves the other three
    /// where they were.**
    ///
    /// The property that makes the layer worth having: a view reading a file is
    /// not a view discarding the player's quality clamp, the antialiasing rung
    /// they picked, or the override game code set a frame ago. Built on the null
    /// backend, which records the descriptors and draws nothing — the same
    /// device `graph_compile`'s renderer tests use, and the only one that runs
    /// with no ICD.
    #[test]
    fn setting_a_camera_stack_leaves_the_other_three_layers_alone() {
        use crcbl_hal::null::NullInstance;
        use crcbl_hal::{AdapterId, DeviceDesc, Format, Instance, QueueKind};

        use crate::effects::{EffectOverride, EffectRequest};

        let instance = NullInstance::gpu_driven();
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("camera stack"),
                ..DeviceDesc::for_adapter(AdapterId(0))
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut renderer =
            crate::ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("the null backend accepts every descriptor");

        // The other three layers, each set to something a default would not
        // produce, so a `set_camera_stack` that replaced the whole request
        // fails here rather than passing on a field that happened to match.
        let elsewhere = EffectRequest {
            camera: RenderEffects::empty(),
            video: RenderEffects::all().difference(RenderEffects::BLOOM),
            antialiasing: Some(Antialiasing::Cmaa2),
            programmatic: EffectOverride::none().force(RenderEffects::CONTACT_SHADOWS, Some(true)),
        };
        renderer.set_effect_request(elsewhere);

        let stack =
            CameraStack::from_ron("(shadows: Some(()), bloom: Some(()))").expect("that is a stack");
        renderer.set_camera_stack(&stack);

        let after = renderer.effect_request();
        assert_eq!(
            after.camera,
            RenderEffects::SHADOWS.union(RenderEffects::BLOOM),
            "the camera layer is the file's",
        );
        assert_eq!(after.video, elsewhere.video, "the video clamp moved");
        assert_eq!(
            after.antialiasing, elsewhere.antialiasing,
            "the player's tier moved",
        );
        assert_eq!(
            after.programmatic, elsewhere.programmatic,
            "the programmatic override moved",
        );

        renderer.destroy(device.as_ref());
    }
}
