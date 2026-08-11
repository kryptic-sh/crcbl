//! Parallax layers: which band of the world a sprite is drawn in, and how much
//! of the camera's motion that band takes.
//!
//! ```text
//!   LayerStack ──push_layer(Parallax)──▶ layer 0 (back) … layer n (front)
//!        │  push(layer, sprite)              each keeps its own submission order
//!        │
//!        └──resolve(camera)──▶ &[Sprite]  back to front, each layer offset by
//!                                          (1 − factor) × camera
//!                                              │
//!                                              ▼
//!                              SpriteRenderer::begin_frame
//! ```
//!
//! # What a parallax factor means
//!
//! **The fraction of the camera's motion the layer takes.** `1.0` is fixed in
//! the world — it scrolls past at the same rate as everything else in the
//! simulation. `0.0` is pinned to the camera and never moves on screen, which is
//! a HUD. `0.5` is a hill that drifts by half as much as the ground in front of
//! it.
//!
//! There is no second matrix and no second pass. A sprite on a layer of factor
//! `p`, at world position `w`, is submitted at `w + (1 − p) · c` for a camera at
//! `c`, so the one view-projection the pass already has puts it on screen at
//! `w − p · c`. At `p = 1` that is `w − c`, the ordinary world position; at
//! `p = 0` it is `w`, unmoved by the camera. The whole feature is one add per
//! sprite in [`LayerStack::resolve`], which is why it does not need a
//! per-layer camera, a second constant buffer, or a pass per layer.
//!
//! # This is not Aseprite's `meta.layers`
//!
//! `docs/specs/crcbl/pix.md` §9 flags the collision by name and it is worth
//! repeating where the type lives. Aseprite's `meta.layers` is a **compositing**
//! layer *inside one sheet* — the art's own stack of paint, which the exporter
//! has already flattened into the frames by the time anything here sees them. A
//! [`Layer`] here is a band of the **world**, chosen by the game per drawn
//! sprite, and two sprites cut from the same sheet routinely belong to different
//! ones. Nothing reads `meta.layers`; nothing here is derived from it.
//!
//! That is also why parallax lives on the *drawn sprite* rather than on
//! [`Sheet`](crcbl_sprite::Sheet): one pipe sheet serves the foreground pipes
//! and, at a different factor, the distant ones.
//!
//! # Ordering, and slice 3's batching contract
//!
//! [`sprite_pass`](crate::sprite_pass) is explicit that **submission order is
//! the caller's** and that nothing sorts behind their back — `A B A` is three
//! batches, not two, because grouping the two `A`s would put the second one on
//! top of `B`. Drawing layers back to front *is* a reordering, so it cannot
//! happen inside the renderer without breaking that.
//!
//! It does not. Two things keep both properties at once:
//!
//! * **A layer is a container, not a field on [`Sprite`].** There is nothing to
//!   sort by, because the sprites of one layer are already contiguous — the
//!   ordering is structural. Nothing compares two sprites, anywhere.
//! * **[`LayerStack::resolve`] is the caller's own call**, named, documented as
//!   the point where the frame's order is decided, and returning the flat slice
//!   they then hand to [`begin_frame`](crate::SpriteRenderer::begin_frame). A
//!   caller who wants their own order does not build a `LayerStack` and nothing
//!   changes for them.
//!
//! Within a layer, order is preserved exactly: `resolve` concatenates, it does
//! not sort. So the batching rule still sees the order it was given, and
//! `sprite_pass`'s existing order tests are untouched — none of them changed for
//! this module.

use crate::sprite_pass::Sprite;

/// How much of the camera's motion a layer takes.
///
/// `1.0` is fixed in the world, `0.0` is pinned to the camera. Values between
/// are the usual parallax; values outside are legal and occasionally wanted — a
/// factor above `1` runs *ahead* of the camera, which is how a foreground
/// element sweeps past faster than the ground.
///
/// ```
/// use crcbl_render::Parallax;
/// // A camera 100 units to the right. Each layer keeps the share of that
/// // movement it did *not* take: the hills drift by half, the HUD by all of it
/// // (so it does not move on screen), the world by none.
/// let hills = Parallax::new(0.5).expect("0.5 is finite");
/// assert_eq!(hills.offset([100.0, 0.0]), [50.0, 0.0]);
/// assert_eq!(Parallax::CAMERA.offset([100.0, 0.0]), [100.0, 0.0]);
/// assert_eq!(Parallax::WORLD.offset([100.0, 0.0]), [0.0, 0.0]);
/// ```
///
/// Deliberately **not** `PartialOrd`: a layer's depth is where it sits in its
/// [`LayerStack`], not how much parallax it takes, and the two disagree the
/// moment a HUD is added.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parallax(f32);

impl Parallax {
    /// Fixed in the world: the layer the simulation actually happens on.
    pub const WORLD: Self = Self(1.0);

    /// Pinned to the camera, so it never moves on screen — a HUD.
    pub const CAMERA: Self = Self(0.0);

    /// A factor of `factor`.
    ///
    /// `None` for anything not finite. A `NaN` factor would put every sprite on
    /// the layer at a `NaN` world position, which draws nothing at all and looks
    /// exactly like a renderer that stopped working; refusing it here is one
    /// branch, at the only point where a computed factor enters the system.
    #[must_use]
    pub fn new(factor: f32) -> Option<Self> {
        factor.is_finite().then_some(Self(factor))
    }

    /// The factor itself.
    #[must_use]
    pub const fn factor(self) -> f32 {
        self.0
    }

    /// How far this layer's contents shift for a camera at `camera`.
    ///
    /// `(1 − factor) × camera`, which is the whole of parallax — see this
    /// module's docs for why that is the value that makes one view-projection
    /// enough.
    #[must_use]
    pub fn offset(self, camera: [f32; 2]) -> [f32; 2] {
        let take = 1.0 - self.0;
        [take * camera[0], take * camera[1]]
    }
}

impl Default for Parallax {
    /// [`Parallax::WORLD`]: a layer nobody said anything about is part of the
    /// world, which is the answer that makes an unconfigured stack behave like
    /// no stack at all.
    fn default() -> Self {
        Self::WORLD
    }
}

/// A layer's place in a [`LayerStack`]: `0` is the **backmost**.
///
/// Opaque and per-stack, for [`SheetId`](crate::SheetId)'s reason: an index from
/// one stack means nothing in another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Layer(usize);

impl Layer {
    /// The depth this layer sits at, `0` being the back. For tests and
    /// diagnostics.
    #[must_use]
    pub const fn depth(self) -> usize {
        self.0
    }
}

/// One layer's parallax factor and the sprites pushed onto it, in order.
#[derive(Clone, Debug, Default)]
struct Band {
    parallax: Parallax,
    sprites: Vec<Sprite>,
}

/// A frame's sprites, grouped into parallax layers and drawn back to front.
///
/// Built once and reused: [`LayerStack::clear`] empties the sprites and keeps
/// the layers and their `Vec`s, so a steady-state frame allocates nothing — the
/// same discipline [`SpriteRenderer`](crate::SpriteRenderer)'s instance ring
/// follows.
///
/// ```
/// use crcbl_hal::null::NullInstance;
/// use crcbl_hal::{DeviceDesc, Format, Instance, QueueKind};
/// use crcbl_render::{LayerStack, Parallax, SheetDesc, Sprite, SpriteRenderer};
///
/// # let instance = NullInstance::gpu_driven();
/// # let adapter = instance.adapters().remove(0);
/// # let device = instance.create_device(&DeviceDesc::for_adapter(adapter.id))?;
/// # let queue = device.queue(QueueKind::Graphics).expect("always present");
/// # let mut renderer =
/// #     SpriteRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)?;
/// # let sheet = renderer.register_sheet(device.as_ref(), &SheetDesc {
/// #     label: "art", width: 1, height: 1,
/// #     sample: crcbl_render::SampleMode::Pixel, pixels: &[255; 4],
/// # })?;
/// # let sprite =
/// #     |x: f32| Sprite::new(sheet, [x, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
/// let mut stack = LayerStack::new();
/// let hills = stack.push_layer(Parallax::new(0.5).expect("finite"));
/// let ground = stack.push_layer(Parallax::WORLD);
/// let hud = stack.push_layer(Parallax::CAMERA);
///
/// // Submitted in whatever order the game finds them; the layer decides depth.
/// stack.push(ground, sprite(0.0));
/// stack.push(hills, sprite(0.0));
/// stack.push(hud, sprite(0.0));
///
/// // The camera has moved 100 units right.
/// let frame = stack.resolve([100.0, 0.0]);
/// // Back to front: hills, then ground, then the HUD.
/// assert_eq!(frame[0].rect[0], 50.0, "the hills took half the camera");
/// assert_eq!(frame[1].rect[0], 0.0, "the ground is where the world put it");
/// assert_eq!(frame[2].rect[0], 100.0, "the HUD follows the camera exactly");
/// # renderer.destroy(device.as_ref());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct LayerStack {
    bands: Vec<Band>,
    /// The flattened frame [`LayerStack::resolve`] hands out, kept between
    /// frames so its capacity is reused.
    flat: Vec<Sprite>,
}

impl LayerStack {
    /// An empty stack with no layers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a layer **in front of** every layer already added, and returns it.
    ///
    /// Depth is the order layers are added, not a number the caller invents:
    /// there is no way to write two layers at the same depth, and no comparison
    /// anywhere that could tie.
    pub fn push_layer(&mut self, parallax: Parallax) -> Layer {
        self.bands.push(Band {
            parallax,
            sprites: Vec::new(),
        });
        Layer(self.bands.len() - 1)
    }

    /// How many layers the stack has.
    #[must_use]
    pub fn layer_count(&self) -> usize {
        self.bands.len()
    }

    /// `layer`'s parallax factor.
    ///
    /// # Panics
    ///
    /// If `layer` came from a different stack — the same contract as indexing a
    /// [`Vec`], and for the same reason: a silently ignored push would lose
    /// sprites rather than report anything.
    #[must_use]
    pub fn parallax(&self, layer: Layer) -> Parallax {
        self.band(layer).parallax
    }

    /// The sprites pushed onto `layer` so far, in submission order and **before**
    /// parallax is applied.
    ///
    /// # Panics
    ///
    /// If `layer` came from a different stack.
    #[must_use]
    pub fn sprites(&self, layer: Layer) -> &[Sprite] {
        &self.band(layer).sprites
    }

    /// Adds one sprite to the front of `layer`'s own order.
    ///
    /// # Panics
    ///
    /// If `layer` came from a different stack.
    pub fn push(&mut self, layer: Layer, sprite: Sprite) {
        self.band_mut(layer).sprites.push(sprite);
    }

    /// Adds several, keeping their order — [`NineQuads::sprites`] feeds straight
    /// into this.
    ///
    /// [`NineQuads::sprites`]: crate::nine_slice::NineQuads::sprites
    ///
    /// # Panics
    ///
    /// If `layer` came from a different stack.
    pub fn extend(&mut self, layer: Layer, sprites: impl IntoIterator<Item = Sprite>) {
        self.band_mut(layer).sprites.extend(sprites);
    }

    /// Empties every layer, keeping the layers themselves and their capacity.
    ///
    /// The per-frame call. [`LayerStack::push_layer`] is startup work — a stack
    /// rebuilt every frame would hand out fresh [`Layer`]s a game then has to
    /// store somewhere anyway.
    pub fn clear(&mut self) {
        for band in &mut self.bands {
            band.sprites.clear();
        }
        self.flat.clear();
    }

    /// How many sprites are in the stack across all layers.
    #[must_use]
    pub fn sprite_count(&self) -> usize {
        self.bands.iter().map(|band| band.sprites.len()).sum()
    }

    /// **Flattens the stack into one frame's submission order.** Back layer
    /// first, each layer's sprites offset by its share of `camera`.
    ///
    /// `camera` is the camera's world position on the sprite plane —
    /// `[camera.eye.x, camera.eye.y]` for the [`Camera`](crate::Camera) the same
    /// frame's `view_projection` came from. Passing a different one silently
    /// desynchronises the parallax from the projection; there is no way for this
    /// to check that, which is why it takes the position rather than pretending
    /// to derive it.
    ///
    /// **This is the reordering, and it is the caller's.**
    /// [`sprite_pass`](crate::sprite_pass) never sorts; this does, once, where
    /// the caller can see it. Within a layer nothing moves: the result is the
    /// layers concatenated in depth order, each in its own push order, so the
    /// batching rule downstream still sees exactly the order it was handed.
    pub fn resolve(&mut self, camera: [f32; 2]) -> &[Sprite] {
        let total = self.sprite_count();
        self.flat.clear();
        self.flat.reserve(total);
        for band in &self.bands {
            let [dx, dy] = band.parallax.offset(camera);
            self.flat.extend(band.sprites.iter().map(|sprite| {
                let mut moved = *sprite;
                moved.rect[0] += dx;
                moved.rect[1] += dy;
                moved
            }));
        }
        &self.flat
    }

    /// The frame [`LayerStack::resolve`] last produced.
    #[must_use]
    pub fn resolved(&self) -> &[Sprite] {
        &self.flat
    }

    fn band(&self, layer: Layer) -> &Band {
        self.bands
            .get(layer.0)
            .unwrap_or_else(|| panic!("layer {} is not in this stack", layer.0))
    }

    fn band_mut(&mut self, layer: Layer) -> &mut Band {
        let count = self.bands.len();
        self.bands
            .get_mut(layer.0)
            .unwrap_or_else(|| panic!("layer {} is not in a stack of {count}", layer.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite_pass::SheetId;

    fn sprite(sheet: SheetId, position: [f32; 2]) -> Sprite {
        Sprite::new(
            sheet,
            [position[0], position[1], 4.0, 4.0],
            [0.0, 0.0, 1.0, 1.0],
        )
    }

    /// Where a sprite lands **on screen** for a camera at `camera`: the
    /// submitted world position minus the camera, which is what the
    /// view-projection does.
    fn screen(sprite: &Sprite, camera: [f32; 2]) -> [f32; 2] {
        [sprite.rect[0] - camera[0], sprite.rect[1] - camera[1]]
    }

    /// **The parallax claim, measured.** For a given camera movement, a layer at
    /// factor 0.5 moves half as far on screen as the world does, and a layer at
    /// 0 does not move at all.
    ///
    /// Asserted on the *change* between two camera positions rather than on one
    /// frame's numbers, because that is what parallax is — a frame's absolute
    /// offset can be right for the wrong reason.
    #[test]
    fn a_half_parallax_layer_moves_half_as_far_as_the_world() {
        let sheet = SheetId(0);
        let mut stack = LayerStack::new();
        let far = stack.push_layer(Parallax::new(0.5).expect("finite"));
        let world = stack.push_layer(Parallax::WORLD);
        let hud = stack.push_layer(Parallax::CAMERA);
        // The *same* world position on all three, so any difference is the
        // factor and nothing else.
        for layer in [far, world, hud] {
            stack.push(layer, sprite(sheet, [10.0, 20.0]));
        }

        let before: Vec<[f32; 2]> = stack
            .resolve([0.0, 0.0])
            .iter()
            .map(|sprite| screen(sprite, [0.0, 0.0]))
            .collect();
        let camera = [100.0f32, -40.0];
        let after: Vec<[f32; 2]> = stack
            .resolve(camera)
            .iter()
            .map(|sprite| screen(sprite, camera))
            .collect();

        let moved: Vec<[f32; 2]> = before
            .iter()
            .zip(&after)
            .map(|(a, b)| [b[0] - a[0], b[1] - a[1]])
            .collect();
        assert_eq!(
            moved[1],
            [-100.0, 40.0],
            "the world layer moves the whole of the camera, the other way"
        );
        assert_eq!(
            moved[0],
            [-50.0, 20.0],
            "and a factor of 0.5 moves exactly half as far"
        );
        assert_eq!(
            moved[2],
            [0.0, 0.0],
            "a factor of 0 is pinned to the camera: a HUD does not move at all"
        );

        // Said the other way round, so a sign error that halved both cannot
        // pass: half of the world layer's movement, not half of something else.
        assert_eq!(moved[0][0], moved[1][0] / 2.0);
        assert_eq!(moved[0][1], moved[1][1] / 2.0);
        // Anti-vacuity: the world layer really did move.
        assert_ne!(moved[1], [0.0, 0.0]);
    }

    /// Resolving is back to front, and **within a layer nothing moves**. That
    /// second half is the batching contract: `sprite_pass` batches consecutive
    /// sprites and never reorders, so a stack that shuffled inside a layer would
    /// silently recompose the frame.
    #[test]
    fn resolve_concatenates_back_to_front_and_preserves_each_layers_own_order() {
        let mut stack = LayerStack::new();
        let back = stack.push_layer(Parallax::WORLD);
        let front = stack.push_layer(Parallax::WORLD);
        assert_eq!((back.depth(), front.depth()), (0, 1));
        assert_eq!(stack.layer_count(), 2);

        // Distinct x per sprite, and interleaved pushes: a stack that appended
        // to one flat list would come back in push order rather than layer
        // order, and one that sorted inside a layer would come back ascending.
        let sheet = SheetId(0);
        for (layer, x) in [
            (front, 3.0),
            (back, 1.0),
            (front, 0.0),
            (back, 2.0),
            (front, 9.0),
        ] {
            stack.push(layer, sprite(sheet, [x, 0.0]));
        }
        assert_eq!(stack.sprite_count(), 5);

        let order: Vec<f32> = stack
            .resolve([0.0, 0.0])
            .iter()
            .map(|sprite| sprite.rect[0])
            .collect();
        assert_eq!(
            order,
            [1.0, 2.0, 3.0, 0.0, 9.0],
            "the back layer's two sprites first, in push order, then the front \
             layer's three, in push order — not sorted, and not interleaved"
        );
        let kept: Vec<f32> = stack.resolved().iter().map(|s| s.rect[0]).collect();
        assert_eq!(kept, order, "the frame is kept for re-reading");

        // The per-layer subsequences are exactly what was pushed there.
        let xs = |layer| {
            stack
                .sprites(layer)
                .iter()
                .map(|s: &Sprite| s.rect[0])
                .collect::<Vec<_>>()
        };
        assert_eq!(xs(back), [1.0, 2.0]);
        assert_eq!(xs(front), [3.0, 0.0, 9.0]);

        // And a resolve leaves the layers themselves alone, so a second one with
        // a different camera starts from the same world positions.
        let again: Vec<f32> = stack
            .resolve([10.0, 0.0])
            .iter()
            .map(|sprite| sprite.rect[0])
            .collect();
        assert_eq!(
            again,
            [1.0, 2.0, 3.0, 0.0, 9.0],
            "a WORLD layer takes no offset, and resolve must not accumulate one"
        );
    }

    /// A layer that is neither `WORLD` nor `CAMERA` still leaves the layers
    /// untouched between frames — the bug an in-place offset would have.
    #[test]
    fn resolving_twice_does_not_accumulate_the_offset() {
        let sheet = SheetId(0);
        let mut stack = LayerStack::new();
        let drift = stack.push_layer(Parallax::new(0.25).expect("finite"));
        stack.push(drift, sprite(sheet, [0.0, 0.0]));

        let first = stack.resolve([80.0, 40.0])[0].rect;
        let second = stack.resolve([80.0, 40.0])[0].rect;
        assert_eq!(first, second, "the same camera must give the same frame");
        assert_eq!(
            [first[0], first[1]],
            [60.0, 30.0],
            "0.75 of the camera, because the layer takes only a quarter of it"
        );
        assert_eq!(
            stack.sprites(drift)[0].rect[0],
            0.0,
            "the layer is untouched"
        );
    }

    /// `clear` is the per-frame call: the layers and their factors survive and
    /// the sprites do not, so a game's [`Layer`] handles stay valid for the
    /// lifetime of the stack rather than for one frame.
    #[test]
    fn clear_keeps_the_layers_and_their_ids() {
        let sheet = SheetId(0);
        let mut stack = LayerStack::new();
        let sky = stack.push_layer(Parallax::new(0.2).expect("finite"));
        let ground = stack.push_layer(Parallax::WORLD);
        stack.extend(sky, (0..4).map(|i| sprite(sheet, [i as f32, 0.0])));
        stack.push(ground, sprite(sheet, [0.0, 0.0]));
        assert_eq!(stack.sprite_count(), 5);
        let count = stack.sprites(sky).len();

        stack.clear();
        assert_eq!(stack.sprite_count(), 0);
        assert_eq!(stack.layer_count(), 2, "the layers survive a clear");
        assert_eq!(stack.parallax(sky).factor(), 0.2);
        assert_eq!(stack.parallax(ground), Parallax::WORLD);
        assert!(stack.resolve([0.0, 0.0]).is_empty());

        // And the ids still address the same layers afterwards.
        stack.extend(sky, (0..count).map(|i| sprite(sheet, [i as f32, 0.0])));
        assert_eq!(stack.sprites(sky).len(), count);
    }

    /// A factor has to be a number. A `NaN` one would put every sprite on the
    /// layer at a `NaN` position, which draws nothing and looks like a broken
    /// renderer rather than a bad input.
    #[test]
    fn a_parallax_factor_must_be_finite() {
        assert_eq!(Parallax::new(0.5).map(Parallax::factor), Some(0.5));
        assert_eq!(Parallax::new(f32::NAN), None);
        assert_eq!(Parallax::new(f32::INFINITY), None);
        assert_eq!(Parallax::new(f32::NEG_INFINITY), None);
        assert_eq!(Parallax::WORLD.factor(), 1.0);
        assert_eq!(Parallax::CAMERA.factor(), 0.0);
        assert_eq!(Parallax::default(), Parallax::WORLD);

        // A factor above 1 runs ahead of the camera, and is allowed.
        let ahead = Parallax::new(2.0).expect("finite");
        assert_eq!(ahead.offset([10.0, 10.0]), [-10.0, -10.0]);
        assert_eq!(Parallax::WORLD.offset([10.0, 10.0]), [0.0, 0.0]);
        assert_eq!(Parallax::CAMERA.offset([10.0, 10.0]), [10.0, 10.0]);
    }

    /// A [`Layer`] from another stack is a programming error, not something to
    /// swallow: a silently dropped push loses sprites and reports nothing.
    #[test]
    #[should_panic(expected = "layer 0 is not in a stack of 0")]
    fn pushing_onto_a_layer_from_another_stack_panics() {
        let sheet = SheetId(0);
        let mut other = LayerStack::new();
        let stray = other.push_layer(Parallax::WORLD);
        LayerStack::new().push(stray, sprite(sheet, [0.0, 0.0]));
    }
}
