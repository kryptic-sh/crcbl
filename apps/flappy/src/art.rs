//! Flappy's art: the baked sheets, the layers they are drawn on, and the one
//! function that turns a frame of simulation into a list of sprites.
//!
//! ```text
//! assets/*.crpix ──build.rs──▶ PNG + sidecar ──include_bytes!──▶ Scene::new
//!                                                                    │
//!  RenderState ─────────────────────── Scene::build(camera) ─────────┤
//!                                                                    ▼
//!                       LayerStack  hills ▸ ground ▸ world   ──▶ &[Sprite]
//!                                                                    │
//!                                                     SpriteRenderer::begin_frame
//! ```
//!
//! # Sprite space is world space times [`TEXELS_PER_UNIT`]
//!
//! **Every rectangle in this module is in texels, not in world units**, and the
//! camera the sprite pass is given is scaled to match. That is not decoration;
//! it is forced by what [`NineSliceSource::expand`] does. A nine-slice's fixed
//! bands are its insets *taken as one target unit per texel* — a 6-texel cap
//! comes out 6 units tall — so a pipe drawn in flappy's world units, where the
//! whole playable band is 12 units, would have a cap taller than the sky. The
//! choice is between an art scale and integer insets of 1, and an inset of 1
//! texel is not a cap.
//!
//! So one constant scales the whole sprite plane. The pipe is then authored at
//! exactly one texel per sprite unit, which is what keeps its cap square while
//! its shaft stretches. [`crate::gpu`] multiplies the camera by the same
//! constant in the same place it builds the projection, so there is one number
//! and no second mapping to disagree with it.
//!
//! # Parallax is derived from the camera the frame actually uses
//!
//! [`Scene::build`] takes the camera position rather than computing one.
//! `gpu.rs`'s header records why: the pipes and the bird only line up if there
//! is a single definition of where the view is, and the first version of the
//! camera went wrong by keeping two. The same value reaches
//! [`crcbl::render::Camera::eye`], [`LayerStack::resolve`] and the tiling
//! arithmetic below, and nothing here reads [`crate::gpu::camera_x`] itself.

use crcbl::hal::{Device, HalError};
use crcbl::render::{
    Layer, LayerStack, NineSliceSource, Parallax, SheetDesc, SheetId, Sprite, SpriteRenderer,
};
use crcbl_sprite::load::{Loaded, load};
use crcbl_sprite::{Playback, Sheet};
use glam::DVec3;

use crate::game::{GAP_HALF_HEIGHT, PIPE_HALF_WIDTH, PipeView, WORLD_CEILING, WORLD_FLOOR};

// `build.rs` writes this: one `*_PNG` and one `*_JSON` per `assets/*.crpix`,
// with the sidecar `None` for art that needs no metadata.
include!(concat!(env!("OUT_DIR"), "/art_data.rs"));

// ---------------------------------------------------------------------------
// The scale, the clock, and the colours
// ---------------------------------------------------------------------------

/// Texels of art per world unit — see the module docs for why this exists.
///
/// 20 is chosen by the pipe: the course makes them `2 * PIPE_HALF_WIDTH` = 1.6
/// world units wide, and `assets/pipe.crpix` is 32 texels wide, so the art is
/// drawn at exactly one texel per sprite unit and its cap cannot be squashed.
/// The bird's 16×16 then lands on 0.8 world units against a collider 0.7
/// across, which is the right way round.
pub const TEXELS_PER_UNIT: f32 = 20.0;

/// The tick rate the sheets' frame holds were baked against.
///
/// **Must equal `build.rs`'s `ART_TICK_HZ`.** A `.crpix` counts holds in ticks
/// and an Aseprite sidecar counts milliseconds; bake converts one way and
/// [`load`] the other, so a mismatch scales every hold silently.
/// [`tests::the_art_bakes_to_the_sheets_it_declares`] asserts the authored
/// number survives the round trip, which is what would catch it.
const ART_TICK_HZ: u32 = 60;

/// The sky, as the clear value for the swapchain.
///
/// **Linear, not sRGB.** The target is an sRGB format, so the clear is encoded
/// on the way in; these are `#6cadE6` put through the sRGB→linear transfer
/// function once, here, rather than looking washed out on screen.
pub const SKY: [f32; 4] = [0.147, 0.420, 0.787, 1.0];

/// "The sheet as authored" — no tinting anywhere in this game.
const UNTINTED: [f32; 4] = [1.0; 4];

/// How much of the camera's motion the distant hills take.
const HILLS_PARALLAX: f32 = 0.35;

/// And the near ground band, which is most of the way to the world's rate but
/// deliberately not all of it — two bands moving at the same speed are one
/// band.
const GROUND_PARALLAX: f32 = 0.85;

/// Background bands are drawn at twice their texel size.
///
/// A hill has to be several world units across to read as a hill, and at
/// [`TEXELS_PER_UNIT`] that is a couple of hundred texels of hand-written rows
/// for a silhouette with four features in it. Doubling the two background
/// bands is the cheaper half of that trade and is invisible at the distance
/// they sit; the **pipe** is deliberately not scaled, because a nine-slice's
/// caps are measured in texels and a scaled one would stretch them.
const BACKGROUND_SCALE: f32 = 2.0;

/// How far the pipes run past the top and bottom of the playable band.
///
/// The camera sees `camera_half_height()` = `WORLD_CEILING + VIEW_MARGIN`, so a
/// pipe that stopped at the ceiling would show its far cap. One world unit of
/// overhang puts that cap off screen and leaves the near one — which is the
/// whole reason `assets/pipe.crpix` is symmetric.
const PIPE_OVERHANG: f64 = 1.0;

/// The most tiles one background band may emit in a frame.
///
/// A bound rather than a belief: the arithmetic below is `camera / width`, and
/// a camera that ever went non-finite would otherwise ask for an unbounded
/// number of sprites rather than drawing a wrong picture.
const MAX_TILES: usize = 64;

// ---------------------------------------------------------------------------
// The pieces
// ---------------------------------------------------------------------------

/// One tiling background band.
#[derive(Clone, Copy, Debug)]
struct BandArt {
    sheet: SheetId,
    layer: Layer,
    uv: [f32; 4],
    /// The tile's size in sprite units — its texels times [`BACKGROUND_SCALE`].
    width: f32,
    height: f32,
    /// The world y of the band's bottom edge, in sprite units.
    bottom: f32,
}

/// The pipe: one sheet and the three-slice cut out of it.
#[derive(Clone, Copy, Debug)]
struct PipeArt {
    sheet: SheetId,
    source: NineSliceSource,
}

/// The bird: one sheet, the clip it plays, and where that clip has got to.
#[derive(Clone, Debug)]
struct BirdArt {
    sheet: SheetId,
    /// Kept whole rather than reduced to four UV rectangles: [`Playback`] is a
    /// bare cursor and asks the *sheet* which frame a tick lands on.
    description: Sheet,
    /// The index of `flap` in [`Sheet::clips`], resolved once at start-up so a
    /// frame does not search by name.
    clip: usize,
    play: Playback,
    /// The bird's vertical velocity as of the last frame, which is the whole of
    /// how this side of the seam knows the button was pressed — see
    /// [`Scene::observe`].
    climb: f64,
}

// ---------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------

/// Everything flappy draws, and the layers it draws them on.
///
/// Built once — registering a sheet is a blocking staging upload — and then
/// [`Scene::build`] per frame, which clears the stack and refills it without
/// allocating.
#[derive(Debug)]
pub struct Scene {
    stack: LayerStack,
    hills: BandArt,
    ground: BandArt,
    pipe: PipeArt,
    bird: BirdArt,
    /// The band the simulation happens on: pipes, then the bird in front of
    /// them.
    world: Layer,
}

impl Scene {
    /// Loads the baked sheets, uploads them, and builds the layer stack.
    ///
    /// # Errors
    ///
    /// [`HalError`] if a sheet upload or a bind group was refused. Nothing here
    /// fails on the *art*: it was parsed, validated and baked by `build.rs`, so
    /// a sheet that will not load is a bug in this repository rather than a
    /// runtime condition, and it panics naming which one.
    ///
    /// # Panics
    ///
    /// If a baked sheet cannot be read back, or does not contain the frames and
    /// clips this module names.
    pub fn new(device: &dyn Device, sprites: &mut SpriteRenderer) -> Result<Self, HalError> {
        let bird = baked("bird", BIRD_PNG, BIRD_JSON);
        let pipe = baked("pipe", PIPE_PNG, PIPE_JSON);
        let hills = baked("hills", HILLS_PNG, HILLS_JSON);
        let ground = baked("ground", GROUND_PNG, GROUND_JSON);

        let bird_sheet = register(device, sprites, "bird", &bird)?;
        let pipe_sheet = register(device, sprites, "pipe", &pipe)?;
        let hills_sheet = register(device, sprites, "hills", &hills)?;
        let ground_sheet = register(device, sprites, "ground", &ground)?;

        // Back to front, and this is the only place the depth order is
        // written down: `LayerStack` has no depth field to disagree with it.
        let mut stack = LayerStack::new();
        let hills_layer = stack.push_layer(parallax(HILLS_PARALLAX));
        let ground_layer = stack.push_layer(parallax(GROUND_PARALLAX));
        let world = stack.push_layer(Parallax::WORLD);

        Ok(Self {
            stack,
            hills: band(hills_sheet, hills_layer, &hills.sheet, 0.0),
            // The ground hangs *below* the floor: its top edge is the line the
            // world's floor is at, so the strip fills the gap between the floor
            // and the bottom of the camera.
            ground: band(
                ground_sheet,
                ground_layer,
                &ground.sheet,
                -(ground.sheet.height as f32) * BACKGROUND_SCALE,
            ),
            pipe: PipeArt {
                sheet: pipe_sheet,
                source: NineSliceSource::from_sheet(&pipe.sheet, 0)
                    .expect("pipe.crpix declares a nine-slice over its one frame"),
            },
            bird: BirdArt {
                sheet: bird_sheet,
                clip: bird
                    .sheet
                    .clips
                    .iter()
                    .position(|clip| clip.name == "flap")
                    .expect("bird.crpix declares a clip called `flap`"),
                description: bird.sheet,
                play: Playback::new(),
                // A parked bird, which is what `WaitingToStart` holds it as.
                climb: 0.0,
            },
            world,
        })
    }

    /// Moves the bird's flap on by whole simulation ticks.
    ///
    /// Ticks, and not the frame's `dt`: `crcbl_sprite::Playback`'s own docs make
    /// the argument, and it is the same one the simulation makes — an animation
    /// on a float clock lands on a different frame at 20 fps than at 240.
    pub const fn advance(&mut self, ticks: u64) {
        self.bird.play.advance(ticks);
    }

    /// Takes this frame's vertical velocity, and starts the wing beat over if
    /// the player just flapped. Answers whether it did.
    ///
    /// # Why a velocity and not a button
    ///
    /// Nothing on this side of the seam sees the button. The renderer is handed
    /// a [`crate::game::RenderState`] — where the bird is and how fast it is
    /// going — because that is the authoritative state the server produced, and
    /// a second path carrying "was the key down this frame" would be a second
    /// answer to a question the simulation has already settled.
    ///
    /// It does not need one. A flap **replaces** `velocity.y` with
    /// [`crate::game::FLAP_SPEED`] rather than adding to it, and the only other
    /// things that touch it are gravity, which subtracts, and the ceiling, which
    /// clamps toward zero from above. So `velocity.y` **rising** between two
    /// frames happens if and only if the player flapped, and that edge is the
    /// signal. Comparing against the previous frame rather than against
    /// `FLAP_SPEED` itself also catches the first flap of a run, which starts
    /// the bird from a park at zero.
    ///
    /// # Why restart rather than let it run
    ///
    /// The clip was free-running: it advanced with ticks and never looked at the
    /// bird, so the wing beat at a constant three times a second whatever the
    /// player did, and the one moment the animation exists to sell — the
    /// down-stroke that lifts the bird — landed wherever the loop happened to
    /// be. [`Playback::restart`] puts the clip back on its first frame, so the
    /// stroke starts on the flap.
    pub fn observe(&mut self, velocity_y: f64) -> bool {
        let flapped = velocity_y > self.bird.climb;
        self.bird.climb = velocity_y;
        if flapped {
            self.bird.play.restart();
        }
        flapped
    }

    /// How many ticks the flap has been advanced by, for the loop's own tests.
    #[cfg(test)]
    #[must_use]
    pub const fn animation_ticks(&self) -> u64 {
        self.bird.play.elapsed()
    }

    /// This frame's sprites, back to front.
    ///
    /// `camera` and `half_width` are **sprite units** — the camera's centre and
    /// half-extent multiplied by [`TEXELS_PER_UNIT`] — and must be the ones the
    /// same frame's view-projection was built from.
    pub fn build(
        &mut self,
        bird: DVec3,
        pipes: &[PipeView],
        camera: f32,
        half_width: f32,
    ) -> &[Sprite] {
        self.stack.clear();

        for band in [self.hills, self.ground] {
            let factor = self.stack.parallax(band.layer).factor();
            let sprites = tiles(band.width, factor, camera, half_width).map(move |x| Sprite {
                sheet: band.sheet,
                rect: [x, band.bottom, band.width, band.height],
                uv: band.uv,
                tint: UNTINTED,
            });
            self.stack.extend(band.layer, sprites);
        }

        let (sheet, source, world) = (self.pipe.sheet, self.pipe.source, self.world);
        for pipe in pipes {
            for target in pipe_targets(pipe) {
                let quads = source.expand(target);
                self.stack.extend(world, quads.sprites(sheet, UNTINTED));
            }
        }

        let bird = self.bird_sprite(bird);
        self.stack.push(world, bird);
        self.stack.resolve([camera, 0.0])
    }

    /// The bird, at the frame its playback has reached.
    fn bird_sprite(&self, bird: DVec3) -> Sprite {
        let clip = &self.description().clips[self.bird.clip];
        let index = self
            .bird
            .play
            .frame_index(self.description(), clip)
            .expect("a validated sheet always has a frame to show");
        let frame = &self.description().frames[index];
        let (width, height) = (frame.rect.w as f32, frame.rect.h as f32);
        Sprite {
            sheet: self.bird.sheet,
            rect: [
                bird.x as f32 * TEXELS_PER_UNIT - width / 2.0,
                bird.y as f32 * TEXELS_PER_UNIT - height / 2.0,
                width,
                height,
            ],
            uv: self
                .description()
                .uv(index)
                .expect("the index came from this sheet"),
            tint: UNTINTED,
        }
    }

    fn description(&self) -> &Sheet {
        &self.bird.description
    }
}

// ---------------------------------------------------------------------------
// Pure geometry — no device, no sheet ids
// ---------------------------------------------------------------------------

/// The two rectangles a pipe fills, in sprite units: the upper one hanging from
/// above the ceiling, the lower one standing from below the floor.
///
/// Both are `[x, y, w, h]` with the **minimum** corner first, which is what
/// [`NineSliceSource::expand`] and [`Sprite::rect`] take.
fn pipe_targets(pipe: &PipeView) -> [[f32; 4]; 2] {
    let scale = f64::from(TEXELS_PER_UNIT);
    let x = ((pipe.x - PIPE_HALF_WIDTH) * scale) as f32;
    let width = (2.0 * PIPE_HALF_WIDTH * scale) as f32;
    let top = ((WORLD_CEILING + PIPE_OVERHANG) * scale) as f32;
    let bottom = ((WORLD_FLOOR - PIPE_OVERHANG) * scale) as f32;
    let gap_top = ((pipe.gap_centre + GAP_HALF_HEIGHT) * scale) as f32;
    let gap_bottom = ((pipe.gap_centre - GAP_HALF_HEIGHT) * scale) as f32;
    [
        [x, gap_top, width, top - gap_top],
        [x, bottom, width, gap_bottom - bottom],
    ]
}

/// The world x of every tile of a band `width` wide that is on screen.
///
/// A sprite submitted at `w` on a layer of factor `p` lands on screen at
/// `w − p·c` — [`crcbl::render::layers`] derives that — so the tiles that
/// matter are the ones with `w` in `p·c ± half_width`. Nothing here reads the
/// layer: the factor is passed in, from the stack that owns it.
fn tiles(width: f32, parallax: f32, camera: f32, half_width: f32) -> impl Iterator<Item = f32> {
    let centre = parallax * camera;
    let (first, last) = if width > 0.0 && centre.is_finite() && half_width.is_finite() {
        (
            ((centre - half_width) / width).floor(),
            ((centre + half_width) / width).floor(),
        )
    } else {
        (0.0, -1.0)
    };
    let count = ((last - first) as isize + 1).max(0) as usize;
    (0..count.min(MAX_TILES)).map(move |step| (first + step as f32) * width)
}

// ---------------------------------------------------------------------------
// Start-up helpers
// ---------------------------------------------------------------------------

/// Decodes one baked sheet.
fn baked(name: &str, png: &[u8], json: Option<&str>) -> Loaded {
    load(png, json, ART_TICK_HZ)
        .unwrap_or_else(|error| panic!("the baked {name} sheet did not load: {error}"))
}

fn register(
    device: &dyn Device,
    sprites: &mut SpriteRenderer,
    label: &str,
    loaded: &Loaded,
) -> Result<SheetId, HalError> {
    sprites.register_sheet(
        device,
        &SheetDesc {
            label,
            width: loaded.image.width,
            height: loaded.image.height,
            sample: loaded.sheet.sample,
            pixels: &loaded.image.pixels,
        },
    )
}

/// A background band from a single-frame sheet, standing with its bottom edge
/// at `bottom` sprite units.
fn band(sheet: SheetId, layer: Layer, description: &Sheet, bottom: f32) -> BandArt {
    BandArt {
        sheet,
        layer,
        uv: description.uv(0).expect("a background sheet has one frame"),
        width: description.width as f32 * BACKGROUND_SCALE,
        height: description.height as f32 * BACKGROUND_SCALE,
        bottom: (WORLD_FLOOR as f32).mul_add(TEXELS_PER_UNIT, bottom),
    }
}

/// A parallax factor written as a literal in this file, which is therefore
/// finite.
fn parallax(factor: f32) -> Parallax {
    Parallax::new(factor).expect("the factors in this module are literals")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::hal::null::NullInstance;
    use crcbl::hal::{DeviceDesc, Format, Instance, QueueKind};
    use crcbl_sprite::{Direction, NineSlice};

    use crate::game::{GAP_CENTRE_RANGE, gap_centre};

    /// Runs `body` against a scene built on the null backend.
    ///
    /// A real [`SpriteRenderer`] rather than a stub, because [`SheetId`] is
    /// opaque outside `crcbl-render` and there is no other way to have one —
    /// which is the point of it being opaque.
    fn with_scene(body: impl FnOnce(&mut Scene)) {
        let instance = NullInstance::tier_a();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut sprites = SpriteRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("the null backend accepts the sprite pipeline");
        let mut scene = Scene::new(device.as_ref(), &mut sprites).expect("the sheets upload");
        body(&mut scene);
        sprites.destroy(device.as_ref());
    }

    /// Frame `index` of a sheet, as its own block of RGBA, cut out of the strip.
    fn frame_pixels(loaded: &Loaded, index: usize) -> Vec<u8> {
        let rect = loaded.sheet.frames[index].rect;
        let stride = loaded.image.width as usize * 4;
        (0..rect.h as usize)
            .flat_map(|row| {
                let start = (rect.y as usize + row) * stride + rect.x as usize * 4;
                loaded.image.pixels[start..start + rect.w as usize * 4].to_vec()
            })
            .collect()
    }

    /// The camera the whole test module measures against, in sprite units.
    fn camera(bird_x: f64, extent: (u32, u32)) -> (f32, f32) {
        (
            crate::gpu::camera_x(bird_x, extent) * TEXELS_PER_UNIT,
            crate::gpu::camera_half_width(extent) * TEXELS_PER_UNIT,
        )
    }

    const EXTENT: (u32, u32) = (960, 720);

    // -----------------------------------------------------------------------
    // The art itself
    // -----------------------------------------------------------------------

    /// **The art is the art that was authored.** Sizes, frame names, holds,
    /// the clip and its direction, and the nine-slice — every one of them a
    /// number written in a `.crpix` and carried through parse, bake and load.
    ///
    /// A test that only checked `bake` returned `Ok` would pass on a blank
    /// image with no frames in it, which is exactly the failure this is for.
    #[test]
    fn the_art_bakes_to_the_sheets_it_declares() {
        let bird = baked("bird", BIRD_PNG, BIRD_JSON);
        assert_eq!((bird.image.width, bird.image.height), (48, 16));
        assert_eq!((bird.sheet.width, bird.sheet.height), (48, 16));
        let names: Vec<&str> = bird.sheet.frames.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["up", "level", "down"]);
        for (index, frame) in bird.sheet.frames.iter().enumerate() {
            assert_eq!(
                frame.rect,
                crcbl_sprite::Rect::new(index as u32 * 16, 0, 16, 16),
                "frame {index} is not a 16x16 cell of the strip"
            );
            assert_eq!(
                frame.hold, 5,
                "the hold authored in ticks did not survive the millisecond \
                 round trip: build.rs and ART_TICK_HZ disagree"
            );
        }
        let flap = bird.sheet.clip("flap").expect("bird.crpix declares `flap`");
        assert_eq!(flap.frames, [0, 1, 2]);
        assert!(flap.looping);
        assert_eq!(flap.direction, Direction::PingPong);
        assert_eq!(flap.steps(), 4, "up, level, down, level");
        assert_eq!(bird.sheet.clip_duration(flap), Some(20));

        let pipe = baked("pipe", PIPE_PNG, PIPE_JSON);
        assert_eq!((pipe.image.width, pipe.image.height), (32, 24));
        assert_eq!(pipe.sheet.frames.len(), 1);
        assert_eq!(
            pipe.sheet.nine,
            Some(NineSlice::new(0, 0, 6, 6)),
            "a three-slice: the pipe stretches vertically only"
        );

        let hills = baked("hills", HILLS_PNG, HILLS_JSON);
        assert_eq!((hills.image.width, hills.image.height), (64, 24));
        assert!(
            HILLS_JSON.is_none(),
            "one still frame with no clip and no nine-slice needs no sidecar"
        );
        let ground = baked("ground", GROUND_PNG, GROUND_JSON);
        assert_eq!((ground.image.width, ground.image.height), (64, 16));

        // Anti-blank: the pipe has transparent shaft margins *and* opaque
        // pixels, and the ground is opaque everywhere. A sheet of zeroes would
        // satisfy every assertion above.
        let alpha = |loaded: &Loaded| -> (usize, usize) {
            let clear = loaded
                .image
                .pixels
                .chunks_exact(4)
                .filter(|p| p[3] == 0)
                .count();
            (clear, loaded.image.pixels.len() / 4 - clear)
        };
        let (clear, opaque) = alpha(&pipe);
        assert!(clear > 0 && opaque > 0, "{clear} clear, {opaque} opaque");
        assert_eq!(alpha(&ground).0, 0, "the ground band has no holes in it");
    }

    /// **The flap's frames differ.** Three identical frames parse, bake, load
    /// and animate exactly as three different ones do, and every other test in
    /// this file passes on them.
    #[test]
    fn the_flap_frames_are_actually_different_pictures() {
        let bird = baked("bird", BIRD_PNG, BIRD_JSON);
        let frames: Vec<Vec<u8>> = (0..3).map(|i| frame_pixels(&bird, i)).collect();
        for frame in &frames {
            assert_eq!(frame.len(), 16 * 16 * 4);
            assert!(
                frame.chunks_exact(4).any(|p| p[3] != 0),
                "a frame with nothing drawn in it"
            );
        }
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            assert_ne!(
                frames[a], frames[b],
                "flap frames {a} and {b} are the same picture, so the wing does \
                 not move"
            );
        }

        // And the difference is in the wing, not somewhere the eye would catch:
        // rows 0-4 are byte-identical across all three.
        let head = |frame: &[u8]| frame[..5 * 16 * 4].to_vec();
        assert_eq!(head(&frames[0]), head(&frames[1]));
        assert_eq!(head(&frames[0]), head(&frames[2]));
    }

    /// **The pipe's caps fit the shortest pipe the course can build.** Below
    /// `minimum_size` the caps shrink in proportion, which is a pipe whose lip
    /// changes size as the gap moves.
    ///
    /// The shortest pipe is derived from the game's own constants and takes no
    /// credit for [`PIPE_OVERHANG`], which is this module's and not the
    /// simulation's.
    #[test]
    fn the_pipe_cap_fits_the_shortest_pipe_the_course_can_make() {
        let pipe = baked("pipe", PIPE_PNG, PIPE_JSON);
        let source =
            NineSliceSource::from_sheet(&pipe.sheet, 0).expect("the sheet declares a nine-slice");
        let minimum = source.minimum_size().1 / TEXELS_PER_UNIT;

        // A gap pushed as far up as `gap_centre` can put it leaves the shortest
        // possible upper pipe between it and the ceiling.
        let shortest = (WORLD_CEILING - GAP_CENTRE_RANGE - GAP_HALF_HEIGHT) as f32;
        assert!(shortest > 0.0, "the course cannot make a pipe at all");
        assert!(
            minimum < shortest,
            "the pipe's two caps need {minimum} world units and the shortest \
             pipe the course makes is {shortest}"
        );

        // Said the other way: at that height the expansion still emits a shaft
        // between the two caps, rather than the four-quad squashed form.
        let quads = source.expand([0.0, 0.0, 32.0, shortest * TEXELS_PER_UNIT]);
        assert_eq!(quads.len(), 3, "cap, shaft, cap");
        assert!(quads[1].rect[3] > 0.0, "the shaft vanished");

        // The course really does reach that extreme, within a texel.
        let reach = (0..4096)
            .map(|index| gap_centre(crate::game::DEFAULT_SEED, index))
            .fold(0.0f64, |worst, centre| worst.max(centre.abs()));
        assert!(
            reach > GAP_CENTRE_RANGE - 0.05,
            "the course only reaches {reach} of {GAP_CENTRE_RANGE}, so the \
             shortest pipe above is not one it builds"
        );
    }

    // -----------------------------------------------------------------------
    // The scene
    // -----------------------------------------------------------------------

    /// Each band goes on its own layer, back to front, and the world layer
    /// carries the course and the bird and nothing else.
    #[test]
    fn every_band_is_submitted_on_the_layer_it_belongs_to() {
        with_scene(|scene| {
            assert_eq!(scene.stack.layer_count(), 3);
            let (camera, half_width) = camera(0.0, EXTENT);
            let pipes = [
                PipeView {
                    x: 12.0,
                    gap_centre: 1.0,
                },
                PipeView {
                    x: 21.0,
                    gap_centre: -2.0,
                },
            ];
            scene.build(DVec3::ZERO, &pipes, camera, half_width);

            let (hills, ground, world) = (scene.hills, scene.ground, scene.world);
            assert_eq!(hills.layer.depth(), 0, "the hills are the backmost band");
            assert_eq!(ground.layer.depth(), 1);
            assert_eq!(world.depth(), 2);

            let on = |layer: Layer| scene.stack.sprites(layer).to_vec();
            assert!(!on(hills.layer).is_empty(), "no hills were drawn");
            assert!(
                on(hills.layer).iter().all(|s| s.sheet == hills.sheet),
                "something that is not a hill is on the hills layer"
            );
            assert!(
                on(ground.layer).iter().all(|s| s.sheet == ground.sheet),
                "something that is not ground is on the ground layer"
            );

            // Three quads per pipe half — cap, shaft, cap — and the bird last so
            // it composites in front of the course.
            let world_sprites = on(world);
            assert_eq!(world_sprites.len(), pipes.len() * 2 * 3 + 1);
            assert!(
                world_sprites[..world_sprites.len() - 1]
                    .iter()
                    .all(|s| s.sheet == scene.pipe.sheet)
            );
            assert_eq!(
                world_sprites.last().expect("non-empty").sheet,
                scene.bird.sheet,
                "the bird must be the last thing pushed, or the pipes cover it"
            );

            // And the resolved frame is those three layers concatenated, back
            // first — which is what the sprite pass draws in order.
            let flat = scene.stack.resolved();
            assert_eq!(
                flat.len(),
                on(hills.layer).len() + on(ground.layer).len() + world_sprites.len()
            );
            assert_eq!(flat[0].sheet, hills.sheet);
            assert_eq!(flat.last().expect("non-empty").sheet, scene.bird.sheet);
        });
    }

    /// **The bands move at the fractions they claim.** Measured as a *change*
    /// between two cameras, because a single frame's offset can be right for
    /// the wrong reason.
    #[test]
    fn the_parallax_bands_move_at_their_stated_fractions_of_the_camera() {
        with_scene(|scene| {
            assert_eq!(scene.stack.parallax(scene.hills.layer).factor(), 0.35);
            assert_eq!(scene.stack.parallax(scene.ground.layer).factor(), 0.85);

            let half_width = camera(0.0, EXTENT).1;
            let pipe = [PipeView {
                x: 0.0,
                gap_centre: 0.0,
            }];
            // Where every sprite of `sheet` lands on screen: the resolved world
            // position minus the camera, which is what the view-projection does.
            let screens = |scene: &mut Scene, at: f32| -> Vec<(SheetId, f32)> {
                scene
                    .build(DVec3::ZERO, &pipe, at, half_width)
                    .iter()
                    .map(|sprite| (sprite.sheet, sprite.rect[0] - at))
                    .collect()
            };

            let step = 100.0f32;
            let before = screens(scene, 0.0);
            let after = screens(scene, step);

            for (sheet, factor) in [
                (scene.hills.sheet, HILLS_PARALLAX),
                (scene.ground.sheet, GROUND_PARALLAX),
                (scene.pipe.sheet, 1.0),
            ] {
                let moved = -factor * step;
                let mut checked = 0;
                for (_, x) in before.iter().filter(|(s, _)| *s == sheet) {
                    // Only tiles that are still comfortably on screen after the
                    // move; one that left the view has no counterpart.
                    if (x + moved).abs() > half_width * 0.9 {
                        continue;
                    }
                    assert!(
                        after
                            .iter()
                            .any(|(s, y)| *s == sheet && (y - (x + moved)).abs() < 1e-2),
                        "a band at {x} should have moved to {} for a factor of \
                         {factor}; the frame holds {:?}",
                        x + moved,
                        after
                            .iter()
                            .filter(|(s, _)| *s == sheet)
                            .map(|(_, y)| *y)
                            .collect::<Vec<_>>()
                    );
                    checked += 1;
                }
                assert!(checked > 0, "nothing of sheet {sheet:?} was checked");
            }

            // Anti-vacuity: the three rates the stack actually holds are
            // genuinely different, so the loop above is not comparing a band
            // with itself at the world's rate.
            let hills = scene.stack.parallax(scene.hills.layer).factor();
            let ground = scene.stack.parallax(scene.ground.layer).factor();
            assert!(
                hills < ground && ground < Parallax::WORLD.factor(),
                "two bands at {hills} and {ground} are one band"
            );
        });
    }

    /// The bird's clip advances with ticks and loops, and the sprite the scene
    /// submits follows it.
    #[test]
    fn the_birds_flap_advances_with_ticks_and_loops() {
        with_scene(|scene| {
            let (camera, half_width) = camera(0.0, EXTENT);
            // No pipes, so the bird is the last sprite in the frame.
            let uv = |scene: &mut Scene| {
                scene
                    .build(DVec3::ZERO, &[], camera, half_width)
                    .last()
                    .expect("the bird is always drawn")
                    .uv
            };
            // Five ticks a showing, ping-pong over three frames: 0 1 2 1, then
            // round again. Twenty ticks a cycle.
            let expected: Vec<usize> = [0usize, 1, 2, 1]
                .iter()
                .flat_map(|frame| std::iter::repeat_n(*frame, 5))
                .collect();
            assert_eq!(expected.len(), 20);

            let sheet = scene.bird.description.clone();
            let uvs: Vec<[f32; 4]> = (0..3)
                .map(|index| sheet.uv(index).expect("three frames"))
                .collect();
            assert_ne!(uvs[0], uvs[1], "the three frames must cut different cells");

            for cycle in 0..2u64 {
                for (tick, frame) in expected.iter().enumerate() {
                    assert_eq!(
                        uv(scene),
                        uvs[*frame],
                        "tick {tick} of cycle {cycle} should show frame {frame}"
                    );
                    scene.advance(1);
                }
            }
            assert_eq!(
                scene.bird.play.elapsed(),
                40,
                "two whole cycles of a twenty-tick clip"
            );

            // Advancing by n in one call is advancing by one n times.
            let single = uv(scene);
            scene.advance(20);
            assert_eq!(uv(scene), single, "a whole period lands back where it was");
            scene.advance(7);
            let jumped = uv(scene);
            for _ in 0..13 {
                scene.advance(1);
            }
            assert_ne!(jumped, uv(scene), "seven ticks in is not twenty ticks in");
        });
    }

    /// **The wing beat starts when the player flaps, and not otherwise.**
    ///
    /// The clip used to be free-running — advanced by ticks, never told about
    /// the bird — so the one thing the animation is for, a down-stroke on the
    /// button, happened wherever the loop had got to. Measured as the *frame
    /// drawn*, not as `Playback::elapsed`, because a restart that did not change
    /// the picture would not be a fix.
    #[test]
    fn the_wing_beat_restarts_on_a_flap_and_not_on_an_idle_tick() {
        with_scene(|scene| {
            let (camera, half_width) = camera(0.0, EXTENT);
            let uv = |scene: &mut Scene| {
                scene
                    .build(DVec3::ZERO, &[], camera, half_width)
                    .last()
                    .expect("the bird is always drawn")
                    .uv
            };
            let first = scene
                .description()
                .uv(0)
                .expect("the flap's first frame exists");

            // Six ticks of gravity: the clip is past its first showing, so
            // "restarted" and "never moved" are different pictures.
            for tick in 0..6u64 {
                scene.observe(-(tick as f64));
                scene.advance(1);
            }
            let drifted = uv(scene);
            assert_ne!(
                drifted, first,
                "the clip never left its first frame, so a restart would prove \
                 nothing"
            );
            assert_eq!(scene.bird.play.elapsed(), 6);

            // Falling further is not a flap.
            assert!(!scene.observe(-7.0), "a tick of gravity read as a flap");
            assert_eq!(uv(scene), drifted, "an idle tick restarted the wing");
            assert_eq!(scene.bird.play.elapsed(), 6, "and did not touch the clock");

            // The velocity jumping upward is, and only on the tick it jumps.
            assert!(
                scene.observe(crate::game::FLAP_SPEED),
                "a flap was not seen"
            );
            assert_eq!(uv(scene), first, "the wing did not beat on the flap");
            assert_eq!(scene.bird.play.elapsed(), 0);

            // The tick after, the bird is still climbing but slower, which is
            // gravity and not a second flap.
            scene.advance(6);
            let mid_beat = uv(scene);
            assert!(!scene.observe(crate::game::FLAP_SPEED - 0.4));
            assert_eq!(
                uv(scene),
                mid_beat,
                "the beat restarted a second time on one press"
            );

            // And a held bird — parked at zero every tick, as `WaitingToStart`
            // holds it — never restarts, which is what stops the wing stuttering
            // on the title screen.
            scene.observe(0.0);
            scene.advance(3);
            let parked = uv(scene);
            for _ in 0..10 {
                assert!(!scene.observe(0.0), "a parked bird flapped");
            }
            assert_eq!(uv(scene), parked);
        });
    }

    /// **The bird holds still on screen while the course slides past it.**
    ///
    /// This is the claim `app.rs`'s `WorldToScreen` used to carry, and the
    /// reason it existed: the bird and the pipes went through two different
    /// mappings, and a disagreement between them would slide the course against
    /// the bird while every static screenshot still looked right. There is one
    /// mapping now — this is the test that says so.
    #[test]
    fn the_bird_holds_its_place_on_screen_while_the_course_slides_past() {
        with_scene(|scene| {
            let pipe_sheet = scene.pipe.sheet;
            let mut previous: Option<(f32, f32)> = None;
            for bird_x in [0.0, 5.0, 50.0, 500.0] {
                let (at, half_width) = camera(bird_x, EXTENT);
                let pipes = [PipeView {
                    x: crate::game::FIRST_PIPE_X,
                    gap_centre: 0.0,
                }];
                let frame = scene.build(DVec3::new(bird_x, 0.0, 0.0), &pipes, at, half_width);
                let bird = frame.last().expect("the bird is last");
                let pipe = frame
                    .iter()
                    .find(|s| s.sheet == pipe_sheet)
                    .expect("the pipe is drawn");
                let (bird_screen, pipe_screen) = (bird.rect[0] - at, pipe.rect[0] - at);

                if let Some((before_bird, before_pipe)) = previous {
                    assert!(
                        (bird_screen - before_bird).abs() < 1e-2,
                        "the bird moved on screen from {before_bird} to {bird_screen}"
                    );
                    assert!(
                        pipe_screen < before_pipe,
                        "a fixed pipe must slide left as the bird advances: \
                         {before_pipe} then {pipe_screen}"
                    );
                }
                previous = Some((bird_screen, pipe_screen));

                // A third of the way across, as the camera promises.
                let fraction = (bird_screen + half_width) / (2.0 * half_width);
                assert!(
                    (0.25..0.35).contains(&fraction),
                    "the bird sits {fraction} across"
                );
            }
        });
    }

    /// A pipe's quads tile exactly the rectangle the course describes, leaving
    /// exactly the gap the simulation collides against.
    #[test]
    fn a_pipes_quads_cover_the_rectangle_the_course_describes() {
        let pipe = PipeView {
            x: 21.0,
            gap_centre: 1.25,
        };
        let scale = TEXELS_PER_UNIT;
        let [upper, lower] = pipe_targets(&pipe);
        assert_eq!(upper[0], (pipe.x - PIPE_HALF_WIDTH) as f32 * scale);
        assert_eq!(upper[2], (2.0 * PIPE_HALF_WIDTH) as f32 * scale);
        assert_eq!(lower[2], upper[2]);

        // The hole between them is the gap the collider uses, to the texel.
        let hole = upper[1] - (lower[1] + lower[3]);
        assert!(
            (hole - (2.0 * GAP_HALF_HEIGHT) as f32 * scale).abs() < 1e-2,
            "the drawn gap is {} texels and the collided one is {}",
            hole,
            (2.0 * GAP_HALF_HEIGHT) as f32 * scale
        );
        // And both halves run past the camera's band, so no cap shows at the
        // far end.
        let visible = crate::gpu::camera_half_height() * scale;
        assert!(
            upper[1] + upper[3] > visible,
            "the upper pipe stops in view"
        );
        assert!(lower[1] < -visible, "the lower pipe stops in view");

        let source = NineSliceSource::from_sheet(&baked("pipe", PIPE_PNG, PIPE_JSON).sheet, 0)
            .expect("a nine-slice");
        for target in [upper, lower] {
            let quads = source.expand(target);
            assert_eq!(quads.len(), 3);
            let area: f32 = quads.iter().map(|q| q.rect[2] * q.rect[3]).sum();
            assert!(
                (area - target[2] * target[3]).abs() < 1e-1,
                "the three quads cover {area} of a {} target",
                target[2] * target[3]
            );
            // The caps keep their authored height whatever the pipe's length.
            assert_eq!(quads[0].rect[3], 6.0, "the far cap is six texels");
            assert_eq!(quads[2].rect[3], 6.0, "and so is the near one");
        }
    }

    /// A band that is asked for an impossible camera draws nothing rather than
    /// an unbounded number of sprites.
    #[test]
    fn tiling_covers_the_view_without_gaps_and_is_bounded() {
        let visible: Vec<f32> = tiles(128.0, 1.0, 0.0, 400.0).collect();
        assert_eq!(
            visible,
            [-512.0, -384.0, -256.0, -128.0, 0.0, 128.0, 256.0, 384.0],
            "the tiles that cover 400 units either side of the origin"
        );
        assert!(visible[0] <= -400.0, "the left of the view is not covered");
        let last = *visible.last().expect("tiles");
        assert!(
            last + 128.0 >= 400.0,
            "the right of the view is not covered"
        );

        // The factor decides which tiles, and nothing else: at 0.5 the band
        // follows half the camera.
        let half: Vec<f32> = tiles(128.0, 0.5, 1000.0, 400.0).collect();
        assert!(half[0] <= 500.0 - 400.0 && half[0] > 500.0 - 400.0 - 128.0);
        assert!(half.windows(2).all(|w| (w[1] - w[0] - 128.0).abs() < 1e-3));

        assert_eq!(
            tiles(0.0, 1.0, 0.0, 400.0).count(),
            0,
            "a band with no width"
        );
        assert_eq!(tiles(128.0, 1.0, f32::NAN, 400.0).count(), 0);
        assert!(
            tiles(1.0, 1.0, 0.0, 1.0e6).count() <= MAX_TILES,
            "an unbounded band"
        );
    }
}
