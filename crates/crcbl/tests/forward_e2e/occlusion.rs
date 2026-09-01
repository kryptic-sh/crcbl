//! The occlusion chain's own gradients, read back where each pass writes them.
//!
//! # Why a picture cannot show it
//!
//! `tests/render_e2e.rs` renders at 256×192, which is the whole reason this
//! suite's siblings exist: a golden at that size cannot resolve a gradient's
//! *shape*. The banding `docs/backlog.md` records was reported off a real
//! display at 1080p, where an occluder drew concentric terraces instead of a
//! falloff, and every golden in the tree was green while it did.
//!
//! # The scene is arithmetic, not geometry
//!
//! Nothing is rasterised here. The depth prepass's image is filled from the
//! host with a handful of constants — a wall at [`WALL_Z`] and surfaces lifted
//! in front of it, [`PLATE_LIFT`] for the occluder the two gradients are read
//! past and [`HALO_LIFT`] for the foreground the reconstruction is read across —
//! so the input to `ssao.slang` is exact and the only thing that can move the
//! output is the shader. A rendered box would put its own silhouette aliasing
//! into the measurement.
//!
//! The passes are built here rather than driven through `ForwardRenderer`
//! because `crcbl_render::ssao::Ssao` is private to that crate, and because a
//! frame would bring the whole lighting stack into a question that is about
//! three full-screen draws. The pipelines and both bind-group layouts are
//! `crates/crcbl-render/src/ssao.rs`'s, copied field for field.
//!
//! # The pair runs small, and so does the measurement
//!
//! `ssao.slang` and `ssao_blur.slang` render into [`OCCLUSION_EXTENT`] — each
//! axis of the scene's own extent divided by
//! `crcbl_shaders::ssao::RESOLUTION_DIVISOR` — while the depth image they read
//! stays at [`EXTENT`], because that is the pair that ships: the gather maps its
//! pixel to a full-resolution texel through `full_res_pixel` and marches there.
//! So the two gradients below are measured in the pixels the gather actually
//! runs one horizon march per, and every constant naming a position in them is
//! in that grid rather than in the scene's.
//!
//! **The two gradient tests stop at the blur, and that is deliberate.** They
//! exist to find terracing in the gather's own dither ladder, and
//! `ssao_upsample.slang` is a bilateral filter that smooths exactly that — a
//! measurement taken through it would be a measurement of the smoothing.
//! [`the_reconstruction_does_not_halo_a_silhouette`] is where that pass is run,
//! at [`EXTENT`], and what it asks about is the reconstruction rather than the
//! march.
//!
//! # What "terraced" is, as a number
//!
//! Along a row crossing the plate's edge the blurred occlusion rises from its
//! darkest value back to unoccluded over about [`SLICE_STEPS_HINT`] steps of the
//! march. With every pixel starting its march at the same fraction of a step,
//! a horizon can only sit at that many distances, so the row is a staircase:
//! long runs of one 8-bit level separated by large jumps. The measure is
//! therefore the **longest run of a single level** inside the gradient, and
//! `ssao.slang`'s `STEP_OFFSETS` is what breaks it.
//!
//! # The other axis, and why it needs a different scene
//!
//! That is the *radial* artefact — how far along a slice a horizon may sit. The
//! **tangential** one is about how many slice *planes* there are, and the wall
//! and plate above cannot show it: the plate spans every row, so the blurred
//! occlusion at a pixel depends on its column alone. `ssao_blur.slang`'s window
//! is `-1..=2` on each axis, which covers each of the sixteen tile phases
//! exactly once, so a column through that scene is constant by construction and
//! a measurement along it would be a check wired to nothing.
//!
//! [`the_tangential_occlusion_line_does_not_step`] therefore tilts the occluder.
//! Its plate is a half-plane whose edge runs along [`EDGE_STEP`], and the line it
//! reads runs **parallel to that edge** — so every sample sits at exactly the
//! same perpendicular distance from the occluder and the continuous answer along
//! it is one constant. The predicate is on integers and the line steps by whole
//! pixels, so neither the scene nor the sampling rounds anything: every level of
//! variation the line carries is the pass's.
//!
//! What varies is the pairing between a blur tap's tile phase and its distance
//! from the edge. A step along the edge moves both `x & 3` and `y & 3`, so it
//! re-pairs all sixteen phases with sixteen different distances and the blur's
//! sum moves. **How far it moves is what the slice count buys**: with two slices
//! a tile spans eight plane orientations and with four it spans twelve —
//! `ssao.slang`'s `SLICE_COUNT_MAX` counts them — and the fewer orientations the
//! neighbourhood holds, the more of the answer rides on which ones this
//! particular pixel drew.

use crate::harness::{Headless, poisoned};
use crcbl::hal::{
    Barriers, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferImageCopy,
    BufferUsage, Capability, ClearValue, ColorAttachment, ColorTargetState, CommandEncoderDesc,
    Device, Extent3d, Features, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, ImageAspect,
    ImageBarrier, ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageType,
    ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType, LoadOp, MemoryLocation,
    MultisampleState, Offset3d, PassTimestampWrites, PipelineLayoutDesc, PipelineLayoutHandle,
    PrimitiveState, QueryKind, QuerySetDesc, Rect2d, RenderPassDesc, ResourceState, SampleType,
    ShaderEntry, ShaderModuleDesc, ShaderStages, StoreOp, SubmitInfo, Viewport,
};
use crcbl::math::{Mat4, Vec4};
use crcbl::render::Projection;
use crcbl::shaders::{SSAO, SSAO_BLUR, SSAO_UPSAMPLE, Shader, Stage, ssao::SsaoParams};

/// The frame this measures at.
///
/// 1080p rather than the golden suite's 256×192, which is the point: the
/// terraces are tens of pixels wide and a frame that small has no room for one.
/// The swapchain the fixture opens is this size too and nothing is drawn into
/// it — every image here is created below.
///
/// The depth prepass is this size, the reconstruction is this size, and the
/// gather and the blur are [`OCCLUSION_EXTENT`].
const EXTENT: (u32, u32) = (1920, 1080);

/// The extent the gather and the blur run at, and the grid both gradients are
/// measured in.
///
/// **`crates/crcbl-render/src/ssao.rs`'s `half_extent`, mirrored**: each axis of
/// [`EXTENT`] divided by `crcbl_shaders::ssao::RESOLUTION_DIVISOR` and rounded
/// **up**, which is the rounding that keeps the two grids covering — every pixel
/// of the scene has a sample at or before it. That function is `pub(crate)` so
/// this cannot call it; the *constant* is reached for rather than a two spelled
/// here, because a second halving written by hand is the drift `ssao.slang`
/// declares the divisor to prevent, and this file would be the copy nothing
/// holds.
///
/// **The depth image stays at [`EXTENT`].** `ssao.slang`'s `full_res_pixel`
/// multiplies this pass's pixel back up to the texel it gathers at, so a target
/// at the scene's own extent sends the march off the end of the prepass image —
/// which is what these tests did on the day the pair was halved.
const OCCLUSION_EXTENT: (u32, u32) = (
    EXTENT.0.div_ceil(crcbl::shaders::ssao::RESOLUTION_DIVISOR),
    EXTENT.1.div_ceil(crcbl::shaders::ssao::RESOLUTION_DIVISOR),
);

/// The vertical field of view the scene is projected through.
///
/// A third of a turn. It and [`WALL_Z`] together set the projected radius,
/// which is the one number that decides whether this scene can terrace at all —
/// see [`WALL_Z`].
const FOV_Y: f32 = core::f32::consts::FRAC_PI_3;

/// The near plane, and under reversed-Z the only number that sets depth
/// precision. `crcbl::render::Projection`'s own default.
const NEAR: f32 = 0.1;

/// View-space distance to the wall, in world units.
///
/// **Close, and that is what makes the measurement possible.** The wall's
/// distance sets [`reach_in_pixels`], and a step of the march is a quarter of
/// it; the plate's silhouette is a straight edge, so a march along
/// `SLICE_DIRECTIONS[i]` crosses it at that direction's own projection onto the
/// row — which already spreads the four step distances over four positions per
/// direction. At a wall ten units away the whole falloff is about forty columns
/// and those positions land within a column or two of each other, so the
/// coherent march this test is about produces no terrace to find: swept on
/// 2026-08-30, the longest run there was four columns *with the offsets forced
/// off*. Bringing the wall in to here makes a step tens of columns wide and the
/// direction spread no longer closes the gaps. See [`MAX_RUN`] for what the two
/// marches then measure.
const WALL_Z: f32 = 2.5;

/// How far in front of the wall the occluding plate stands, in world units.
///
/// Well inside [`RADIUS`], so a tap that lands on the plate is inside the
/// neighbourhood the integral counts rather than rejected by the falloff.
const PLATE_LIFT: f32 = 0.15;

/// How far in front of the wall the halo test's foreground bar stands, in world
/// units.
///
/// **Past both filters' depth tolerance, which is what makes it a silhouette
/// rather than a fold.** `ssao_blur.slang` and `ssao_upsample.slang` share a
/// `DEPTH_TOLERANCE_RADII` of two, so a tap whose surface is more than two
/// [`RADIUS`]-lengths from the pixel's takes no share at all; this is two and a
/// half of them, so the reconstruction's ramp is at zero across this edge and
/// not merely low. [`PLATE_LIFT`] is deliberately the opposite case — inside the
/// radius, so the plate occludes the wall — and the two together are what give
/// this scene an occluded background *and* a foreground the filters must not
/// mix it into.
///
/// Written as a multiple of [`RADIUS`] rather than as a distance, because that
/// is what it is: `ssao_blur.slang` declares `DEPTH_TOLERANCE_RADII = 2.0` and
/// `crcbl_shaders::ssao`'s `the_two_occlusion_filters_share_one_depth_tolerance`
/// is what holds `ssao_upsample.slang`'s copy of it to that one. Move the radius
/// and this edge moves with the tolerance rather than crossing it.
const HALO_LIFT: f32 = RADIUS * 2.5;

/// The screen columns the halo test's foreground bar covers, as `start..end`.
///
/// **Depth-image columns**, like [`PLATE_COLUMNS`], and placed just past that
/// plate's right edge so the wall the bar stands against is the darkest part of
/// the falloff — the halo is the difference between the two surfaces, so a bar
/// against unoccluded wall would have nothing to draw.
///
/// **The start is odd on purpose.** `ssao_upsample.slang` taps the sample a
/// pixel sits on and the next one along each axis, so a bar starting on an even
/// column has its first pixel land exactly on a sample of its own and reads one
/// tap: no neighbour, no halo, nothing measured. An odd start puts the bar's
/// first pixel between a wall sample and a bar sample, which is the pairing the
/// reconstruction has to resolve by depth.
const HALO_COLUMNS: core::ops::Range<u32> = 965..1005;

/// Wall samples read on the near side of the bar, for the contrast the halo
/// would be made of: columns left of [`HALO_COLUMNS`], rows above
/// [`HALO_ROWS`].
///
/// Four, which is clear of the bar's own reconstruction — the taps reach one
/// sample forward and no further — and still inside the darkest part of the
/// falloff. The same four on either axis, because it is the same reach measured
/// in the same pixels.
const HALO_MARGIN: u32 = 4;

/// The row the reconstruction is read along, in [`EXTENT`]'s pixels.
///
/// The middle of the frame, as [`LINE_Y`] is in the gather's. The bar spans
/// every row, so the halo this test is about is a property of the column alone
/// and one row carries all of it.
///
/// **On the gather's own grid, which is what makes it one axis.** An even row
/// leaves `ssao_upsample.slang`'s `offset.y` — and so its `span.y` — at zero,
/// so every tap the halo test takes is displaced in `x` alone. [`HALO_LINE_X`]
/// is the same statement with the axes exchanged, and the two of them are why
/// the pair of halo tests is one measurement run once per axis rather than two
/// frames that happen to differ.
const HALO_LINE_Y: u32 = EXTENT.1 / 2;

/// The screen rows [`the_reconstruction_does_not_halo_a_silhouette_down_a_column`]'s
/// foreground bar covers, as `start..end`.
///
/// **[`HALO_COLUMNS`] turned a quarter turn.** The bar spans every column where
/// that one spans every row, and it sits the same distance past
/// [`PLATE_ROWS`]'s lower edge that that range sits past [`PLATE_COLUMNS`]'s
/// right one — the same distance, in the same pixels, because the projected
/// radius is the same count of them on both axes and so is the falloff it sets.
/// It is as tall as that range is wide for the same reason.
///
/// **The start is odd on purpose**, which is [`HALO_COLUMNS`]'s clause on the
/// other axis: a bar starting on an even row has its first pixel land on a
/// sample of its own and reads one tap, so no tap is rejected and the
/// measurement is a check wired to nothing. The test asserts that parity rather
/// than trusting this number to keep it.
///
/// **The far edge is not a second measurement.** The last row is even, so it
/// sits on the gather's grid and reads its own sample alone — the same shape as
/// [`HALO_COLUMNS`], whose last column does too. One edge is measured on each
/// axis, and see the test's header for what that leaves out.
const HALO_ROWS: core::ops::Range<u32> = 545..585;

/// The column the reconstruction is read down, in [`EXTENT`]'s pixels.
///
/// The middle of the frame, as [`HALO_LINE_Y`] is on the other axis. Where that
/// one is a choice about which of many equivalent rows to read, this one is
/// barely a choice at all: [`PLATE_ROWS`] and [`HALO_ROWS`] both span every
/// column, so every column of the frame carries the same falloff and the same
/// silhouette.
///
/// **On the gather's own grid** — see [`HALO_LINE_Y`], which is the same clause
/// about the same parity on the other axis. `offset.x` is zero down this
/// column, so every tap this measurement takes is displaced in `y` alone and
/// what it reports is `ssao_upsample.slang`'s `y` axis by itself.
const HALO_LINE_X: u32 = EXTENT.0 / 2;

/// The screen columns the plate covers, as `start..end`.
///
/// Far wider than the projected radius so the plate's own two edges never
/// interact, and clear of the frame's left border for the same reason.
///
/// **Depth-image columns**, unlike almost every other position in this file:
/// this one describes the scene, and the scene is what the prepass holds at
/// [`EXTENT`]. Where the falloff test needs the pixel of the gather that stands
/// past this edge it divides, with `full_res_pixel`'s rounding — see
/// [`OCCLUSION_EXTENT`].
const PLATE_COLUMNS: core::ops::Range<u32> = 640..960;

/// The screen rows the occluder of the column-wise halo scene covers, as
/// `start..end`.
///
/// **[`PLATE_COLUMNS`] turned a quarter turn**, and the plate of one scene
/// only: [`the_reconstruction_does_not_halo_a_silhouette_down_a_column`] needs
/// an occluder whose edge runs the other way, and the upright plate the rest of
/// this file reads past cannot be it. Depth-image rows, for that constant's
/// reason.
///
/// Its far edge is the frame's centre row, as that range's is the frame's
/// centre column, and it covers as many rows as that one covers columns — which
/// is the same argument rather than a resemblance: the span is chosen against
/// the projected radius so the plate's own two edges never interact, and the
/// projected radius is the same count of pixels on both axes because the pixels
/// are square. Written as that span rather than as a second number for the same
/// length.
const PLATE_ROWS: core::ops::Range<u32> =
    EXTENT.1 / 2 - (PLATE_COLUMNS.end - PLATE_COLUMNS.start)..EXTENT.1 / 2;

/// The row the falloff is read along, in [`OCCLUSION_EXTENT`]'s pixels — the
/// middle of the frame, far from every border.
const LINE_Y: u32 = OCCLUSION_EXTENT.1 / 2;

/// The sampling radius, in world units.
///
/// `crcbl_render`'s `SSAO_RADIUS`, which is private to that crate. The value is
/// mirrored rather than reached for because this test is about the *shape* of
/// the falloff and would measure the same shape at any radius; what it must not
/// do is measure a radius nobody ships.
const RADIUS: f32 = 0.5;

/// `ssao.slang`'s `SLICE_STEPS`, for the failure message's arithmetic.
///
/// Not read by the shader — the shader owns its own copy — and not asserted
/// against it either: this is only how many terraces a reader should expect the
/// baseline to have, and the assertion below is on the runs themselves.
const SLICE_STEPS_HINT: u32 = 4;

/// The `R8Unorm` texel a fully unoccluded pixel carries.
const UNOCCLUDED: u8 = 0xFF;

/// Columns of the gradient skipped at the plate's silhouette.
///
/// `ssao_blur.slang` weights its kernel by view-space depth, so within its own
/// footprint of the plate's edge it rejects taps and its divisor falls towards
/// one. Those columns are a different measurement — the tile banding the
/// backlog's second hypothesis is about — and they are not what this test is
/// for.
///
/// **Unmoved by the halving, because the footprint it covers did not move.**
/// The blur's kernel is `-1..=2` in the pixels the pair runs at, so it is four
/// of them wide whatever those pixels stand for on screen — and this is counted
/// in the same pixels. The 2026-09-02 sweep is what says the skip still lands
/// past the disturbance: the column on the edge reads 188 where its neighbour
/// reads 164, and from the fourth column on the falloff rises monotonically.
const SILHOUETTE_SKIP: u32 = 4;

/// Columns of gradient the run is measured over, starting past
/// [`SILHOUETTE_SKIP`].
///
/// **Most of the falloff, and deliberately not all of it.** At [`WALL_Z`] the
/// 2026-09-02 sweep found the occlusion back at unoccluded eighty-three columns
/// past the plate's edge, and a
/// window run out to there would be measuring the 8-bit channel holding one
/// level across flat wall rather than the march. The test also asserts the
/// window's last column is still occluded, so a change that shortened the
/// falloff fails here rather than quietly measuring that wall.
///
/// **Widened rather than halved on 2026-09-02**, which is the opposite of what
/// halving the pass suggests and is what the sweep said. The coherent march's
/// terraces grow towards the shallow end of the falloff — at this width its
/// worst run is twelve columns against a forty-column window's seven — while
/// the dithered march holds at four either way. See [`MAX_RUN`] for the table.
const WINDOW: u32 = 60;

/// The smallest spread of levels the window must hold.
///
/// Anti-vacuity. A window with no gradient in it has no runs to break either,
/// so every assertion below would pass on a pass that wrote one constant — the
/// exact shape of a check wired to nothing. The 2026-09-02 sweep measured a
/// swing of 75 levels with `STEP_OFFSETS` and 66 without, on radv and on
/// lavapipe alike; this sits well below the smaller of them.
const MIN_SWING: u8 = 45;

/// The longest run of one 8-bit level the window may hold.
///
/// **Swept before it was fixed**, at [`OCCLUSION_EXTENT`] on 2026-09-02, over
/// [`WINDOW`] columns past [`SILHOUETTE_SKIP`], on the two drivers this machine
/// has:
///
/// ```text
///                                        radv   lavapipe
/// every STEP_OFFSETS entry forced to 1     12         12
/// STEP_OFFSETS as `ssao.slang` ships it     4          4
/// ```
///
/// So this sits between them with room on both sides: the dithered march has to
/// grow its worst run by three quarters to go red, and the coherent one has to
/// halve its best to pass. The remaining runs are the blur's footprint — four of
/// the pair's own pixels — which is the width one level can survive however well
/// the march is dithered.
///
/// **The whole table moved when the pair was halved**, and towards each other:
/// at [`EXTENT`] on 2026-08-30 the same two configurations measured 16 and 5 on
/// radv, 13 and 4 on lavapipe, over an eighty-column window. A terrace is a
/// number of pixels of *the pass*, and there are half as many of them across
/// the same falloff — so a bound carried over from that sweep would have passed
/// on both configurations and checked nothing.
const MAX_RUN: usize = 7;

/// The occluder's tilt, as the whole-pixel step that runs **along** its edge.
///
/// Three across and two down. Not the frame's own diagonal, and that is the
/// whole of the choice: a 45 degree edge is exactly the one a slice turned an
/// eighth of a turn runs parallel to, so measuring the eighth turn against it
/// would say more about the alignment than about the turn. Reduced modulo a half
/// turn this direction is in neither the eight orientations `ssao.slang`'s table
/// gives at two slices nor the twelve it gives at four, so neither count gets to
/// sweep straight along the edge.
///
/// **Whole pixels is what keeps the line exact.** A step of `(3, 2)` leaves
/// [`across_edge`] unchanged by construction, so every sample sits at the same
/// perpendicular distance from the same straight edge with nothing rounded and
/// nothing interpolated — see this module's header.
///
/// **Unmoved by the halving, and every clause above is why.** This is a
/// *direction*, and the two grids differ by a uniform scale on both axes: three
/// across and two down in the gather's pixels is six across and four down in the
/// depth image's, which is the same direction and therefore the same angle
/// against the same slice table. The invariance of [`across_edge`] is a property
/// of the ratio and holds on either grid for the same reason. And the tile is
/// indexed by the pass's own pixel, so a step of `(3, 2)` still walks
/// `x & 3` and `y & 3` on the same four-step cycle — the line runs through the
/// same four of the sixteen dither phases it ran through before the pair was
/// halved, in the same cyclic order and entered at a different one of them. What
/// did move is [`DIAGONAL_OFFSET`], because that is a distance and not a
/// direction.
const EDGE_STEP: (i64, i64) = (3, 2);

/// How far the tangential line sits from that edge, in [`across_edge`]'s units.
///
/// **In the edge's own units and not in pixels**, because that is what makes it
/// exact: the perpendicular distance is this over the length of [`EDGE_STEP`],
/// and a constant written in pixels would have to be turned back into something
/// the depth predicate could compare against.
///
/// **In the gather's grid**, which is where the line is read and therefore where
/// [`across_edge`] is evaluated for it — the same edge measured in the depth
/// image's pixels is `crcbl_shaders::ssao::RESOLUTION_DIVISOR` times this. It
/// halved with the pair on 2026-09-02, which keeps the line at the same place on
/// screen and therefore at the same place in the falloff.
///
/// It puts the line well inside the falloff — the test asserts the projected
/// radius at [`WALL_Z`] covers it — and clear of the few columns at the
/// silhouette where `ssao_blur.slang` starts rejecting taps, which is
/// [`SILHOUETTE_SKIP`]'s territory and a different measurement.
const DIAGONAL_OFFSET: i64 = 76;

/// The tangential line's first pixel, `(x, y)`.
///
/// In [`OCCLUSION_EXTENT`]'s pixels, on the wall side of the edge, and far
/// enough from every border that [`DIAGONAL_LENGTH`] steps of [`EDGE_STEP`] stay
/// inside that extent. The test asserts its distance is [`DIAGONAL_OFFSET`]'s
/// rather than this being two numbers that agree today.
const DIAGONAL_FROM: (u32, u32) = (37, 50);

/// Samples taken along the tangential line, one per step of [`EDGE_STEP`].
///
/// The tile phase this walks repeats every four steps, so this is sixty cycles
/// of it — enough that the largest step between neighbours is the tile's worst
/// rather than whichever phase the line happened to start on.
///
/// **Halved with the pair**, because it is a count of the gather's pixels and
/// the line has to stay inside [`OCCLUSION_EXTENT`]: from [`DIAGONAL_FROM`] this
/// ends at `(754, 528)`, within three columns of where the line ended before the
/// halving once that pixel is taken back to the depth image's grid — the same
/// span of the frame, sampled every second pixel of it.
const DIAGONAL_LENGTH: u32 = 240;

/// The difference between two neighbouring samples that counts as a sharp edge,
/// in 8-bit levels.
///
/// Two, because one is what an `R8Unorm` channel does on its own: the true
/// answer along the line is not quite a constant — the view direction turns
/// across a 1080p frame, so the integral's geometry does change — and a
/// slowly-varying quantity in eight bits rolls over by one level wherever it
/// crosses a boundary. A step of two is not the quantiser.
const SHARP_EDGE: u8 = 2;

/// Sharp edges the tangential line may hold at the counts that ship.
///
/// **Swept before it was fixed**, at [`OCCLUSION_EXTENT`] over
/// [`DIAGONAL_LENGTH`] samples on 2026-09-02, on the two drivers this machine
/// has. The anti-baseline is a `SLICE_DIRECTIONS` whose sixteen entries are all
/// the same vector — one plane orientation across the whole tile instead of
/// eight — which is the shortage this measurement is about:
///
/// ```text
///                                          radv   lavapipe
/// every SLICE_DIRECTIONS entry the same     118        113
/// SLICE_DIRECTIONS as `ssao.slang` ships     37         38
/// ```
///
/// So this sits between them on a log scale, with room on both sides: the
/// shipping table has to step two thirds more often to go red, and a table down
/// to one orientation has to lose two fifths of its edges to pass.
///
/// **The shipping count is thirty-seven and not the one it was** before the pair
/// was halved, and the reason is the scene rather than the slices: the blur's
/// footprint spans twice as much of the frame now, so each of the sixteen tile
/// phases is paired with a wider spread of distances from the edge and the sum
/// moves further between neighbours. The gap to the anti-baseline is what this
/// bound is placed inside, and that gap survived — three times rather than a
/// hundred, and still on a log scale.
const MAX_SHARP_EDGES: usize = 64;

/// Samples of the tangential line a failure message prints.
///
/// The line is [`DIAGONAL_LENGTH`] long and the artefact repeats with the tile,
/// so a few cycles of it says everything a reader needs and a full dump would
/// bury it.
const MESSAGE_SAMPLES: usize = 32;

/// The deepest the reconstruction may dip inside the foreground bar, in 8-bit
/// levels.
///
/// **Swept before it was fixed**, at [`EXTENT`] on 2026-09-02, on the two
/// drivers this machine has, and **once per axis**: [`assert_no_halo`] is read
/// by both halo tests, so a bound they share is one that had to be measured in
/// both scenes. Two anti-baselines, because the edit that haloes on one axis
/// and leaves the other exactly right is not the edit that haloes on both:
///
/// * `ssao_upsample.slang` with its depth ramp replaced by a constant one — a
///   plain bilinear upsample, and exactly the substitution
///   `crcbl_shaders::ssao`'s `the_reconstruction_weighs_a_tap_by_its_depth`
///   records nothing else in this workspace noticing;
/// * its `span` taking one lane's `offset` for both — the transposed index
///   [`the_reconstruction_does_not_halo_a_silhouette_down_a_column`] exists for,
///   which drops the second tap on the *other* axis and leaves this one
///   untouched. Run both ways round, and the two tests answer it as
///   complements: `offset.x` for both lanes reddens the column test alone, and
///   `offset.y` for both reddens the row test alone.
///
/// ```text
///                                       radv   lavapipe
/// across a row, the ramp made constant    45         44
/// across a row, span taking offset.y      89         89
/// across a row, as it ships                0          0
/// down a column, span taking offset.x     89         89
/// down a column, as it ships               0          0
/// ```
///
/// **Zero, exactly, and that is the shape of the thing.** A tap across this
/// silhouette is more than [`HALO_LIFT`]'s two and a half radii away, so its
/// share is not small but nil, and the reconstruction returns the bar's own
/// sample unchanged. The bound is four rather than nought so a channel rounding
/// its last level does not go red on arithmetic; the shallowest of the three
/// anti-baselines still has to lose nine tenths of its rim to pass.
///
/// **One sample deep, and the same on both axes.** Each transposition put its
/// whole dip in the bar's first sample and left every other sample of the line
/// at unoccluded, on radv and on lavapipe alike — the taps reach `nearest` and
/// `nearest + 1` and no further, so only the sample standing between the two
/// grids can see the other surface. Which is why each of the pair measures one
/// column and one row respectively, and why a larger `RESOLUTION_DIVISOR` would
/// widen both together.
const MAX_HALO: u8 = 4;

/// The least the bar must stand out from the wall behind it, in 8-bit levels.
///
/// Anti-vacuity, on [`MIN_SWING`]'s terms: a halo is one surface's occlusion
/// drawn onto another, so a wall as bright as the bar in front of it has nothing
/// to draw and would pass [`MAX_HALO`] however the taps were weighted.
///
/// **Swept in both halo scenes**, on 2026-09-02, on radv and on lavapipe alike
/// — the two drivers agreed to the level in each. Across a row the contrast is
/// 89, an unoccluded bar at 255 against wall at 166; down a column it is 97,
/// the same bar against wall at 158. The wall is the darker one there because a
/// silhouette running the other way is not the same shape to the horizon
/// search — `ssao.slang` sweeps a fixed table of slice directions, and it is
/// not isotropic. This sits well below the smaller of the two.
const MIN_HALO_CONTRAST: u8 = 60;

/// The one depth-image column the reconstruction's sliver stands in.
///
/// **Narrower than the occlusion grid and standing off it, which is the whole
/// of the scene.** `ssao.slang` runs one invocation per block of
/// `crcbl::shaders::ssao::RESOLUTION_DIVISOR` pixels on each axis and marches
/// its horizons from the one texel `full_res_pixel` maps that block to, so a
/// surface one column wide that misses every one of those texels has no sample
/// of its own anywhere in the gathered channel. The test asserts that placement
/// off the divisor rather than trusting this number to keep the parity it has.
///
/// **Past [`PLATE_COLUMNS`] and inside the plate's falloff**, so the wall the
/// sliver's nearer tap reads is darkly occluded rather than the unoccluded white
/// the sky beyond it gathers — see [`MIN_SLIVER_TAP_CONTRAST`], which is what
/// that darkness is for. Clear of the few columns at the plate's own silhouette
/// where the filters start rejecting taps, which is [`SILHOUETTE_SKIP`]'s
/// territory and a different measurement.
const SLIVER_COLUMN: u32 = 969;

/// How far either side of [`HALO_LINE_Y`] [`SLIVER_ROWS`] reaches.
///
/// Two rows would cover both parities. This is many times that, because the
/// equality is asserted per row and a band is what would name the row that
/// disagreed — and because it walks `ssao.slang`'s dither tile over and over, so
/// a phase of that tile which broke the reconstruction has nowhere in the band
/// to hide.
const SLIVER_ROW_REACH: u32 = 32;

/// The rows the sliver is read along, as `start..end` in [`EXTENT`]'s pixels.
///
/// **Both row parities, deliberately.** `ssao_upsample.slang` takes one tap per
/// axis where the two grids coincide and two where they do not, so an even row
/// of the sliver drives the two-tap path and an odd row the four-tap one. Every
/// tap of both is rejected in this scene — the extra pair lands on the same two
/// columns one row-block down — and a band over both parities is what says so
/// about both.
///
/// Centred on [`HALO_LINE_Y`], and a band rather than the one row the halo test
/// reads because both parities are wanted — **not** because the level varies
/// along it. It does not: the 2026-09-02 sweep found the whole band reading one
/// value on radv and on lavapipe alike, which is `ssao_blur.slang` having
/// flattened the dither tile by the time the reconstruction taps it, and this
/// scene has no other reason to change down a column. The assertion is against
/// the gathered channel read back from the same device rather than against a
/// level written here, so a constant band is a constant equality and not a
/// weaker one.
const SLIVER_ROWS: core::ops::Range<u32> =
    HALO_LINE_Y - SLIVER_ROW_REACH..HALO_LINE_Y + SLIVER_ROW_REACH;

/// The least the sliver's two taps must differ by, in 8-bit levels.
///
/// Anti-vacuity, on [`MIN_HALO_CONTRAST`]'s terms, and it is what makes the
/// equality this test asserts discriminating. The reconstruction has to answer
/// with the *nearest* tap's own level; a scene whose other tap carried the same
/// level would satisfy that whatever share the other tap was given, so the
/// measurement would pass on a reconstruction that had stopped rejecting
/// anything. Swept at [`OCCLUSION_EXTENT`] on 2026-09-02, on the two drivers
/// this machine has: across [`SLIVER_ROWS`] the two taps sit 86 levels apart at
/// their closest — occluded wall at 169 against the sky's unoccluded 255 — the
/// same on radv and on lavapipe, and this sits well under it.
const MIN_SLIVER_TAP_CONTRAST: u8 = 60;

/// The full-resolution columns the intensity curve is measured over.
///
/// The falloff [`the_blurred_occlusion_falloff_does_not_terrace`] reads, in the
/// reconstruction's own pixels rather than the gather's: [`SILHOUETTE_SKIP`]
/// columns of the pair's grid past the plate's edge, then [`WINDOW`] of them,
/// each multiplied up by `crcbl_shaders::ssao::RESOLUTION_DIVISOR`. The skip is
/// there for that constant's reason — the columns inside the filters' footprint
/// of the silhouette are a different measurement — and the window is wide
/// enough that the curve is checked across most of the falloff's range rather
/// than at one visibility.
const INTENSITY_COLUMNS: core::ops::Range<u32> = PLATE_COLUMNS.end
    + SILHOUETTE_SKIP * crcbl::shaders::ssao::RESOLUTION_DIVISOR
    ..PLATE_COLUMNS.end + (SILHOUETTE_SKIP + WINDOW) * crcbl::shaders::ssao::RESOLUTION_DIVISOR;

/// How far a reconstructed level may sit from the power curve the intensity
/// asked for, in 8-bit levels.
///
/// **The measurement is per column against a prediction**, not a direction: the
/// shader raises the visibility it reconstructed to the exponent, so the
/// unoccluded-by-`INTENSITY_DEFAULT` reading at a column predicts every other
/// reading at that column exactly, up to what the channel can hold. Two
/// roundings are in that comparison — the reading the prediction is made from
/// and the reading it is compared against — and the first is amplified by the
/// exponent's own slope, so a couple of levels at `INTENSITY_MAX` is the floor
/// rather than a tolerance that could be tightened. Swept at [`EXTENT`] on
/// 2026-09-02, on the two drivers this machine has: the worst column of the
/// window was 2.2 levels off on radv and 2.4 on lavapipe. A curve that is the
/// wrong shape misses by tens — applying the exponent to the occlusion rather
/// than to the visibility takes the darkest column of this window to zero.
const MAX_CURVE_ERROR: f64 = 4.0;

/// The least the window's most occluded column must move between the default
/// intensity and each end of the range, in 8-bit levels.
///
/// Anti-vacuity, on [`MIN_SWING`]'s terms and it is the whole point of the test:
/// a knob that reached the shader and was thrown away would leave every reading
/// identical, and [`MAX_CURVE_ERROR`] alone passes on that frame — the curve at
/// the default predicts itself, at every intensity, exactly.
///
/// **The darkest column rather than the window's mean**, because the mean is
/// mostly wall: the exponent moves a visibility of one not at all, so a
/// statistic over the whole falloff is dominated by the columns the knob is
/// least able to move. Swept at [`EXTENT`] on 2026-09-02, on the two drivers
/// this machine has: the darkest column reads 175 at the default and moves +57
/// at `INTENSITY_MIN` and -118 at `INTENSITY_MAX`, the same on radv and on
/// lavapipe. This sits under half of the smaller of them.
const MIN_INTENSITY_SWING: f64 = 30.0;

/// The projected radius at the wall, in pixels — `occlusion_at`'s `reach`.
///
/// The same arithmetic the shader does, on the host: project the two ends of a
/// world-space radius and measure the gap in pixels.
fn reach_in_pixels(projection: Mat4, size: (f32, f32)) -> f32 {
    let centre = Vec4::new(0.0, 0.0, -WALL_Z, 1.0);
    let offset = Vec4::new(RADIUS, 0.0, -WALL_Z, 1.0);
    let near = projection * centre;
    let far = projection * offset;
    (far.x / far.w - near.x / near.w).abs() * 0.5 * size.0
}

/// The reversed-Z depth of a surface at `view_z`, through `projection`.
///
/// Derived from the matrix rather than from a closed form, so the depth the
/// image holds and the `inv_proj` the shader unprojects with cannot disagree.
fn depth_of(projection: Mat4, view_z: f32) -> f32 {
    let clip = projection * Vec4::new(0.0, 0.0, view_z, 1.0);
    clip.z / clip.w
}

/// The reversed-Z depth of a surface standing `lift` world units in front of
/// the wall, checked against the wall's own.
///
/// Every occluder in this file is one of these, and the check is the same for
/// all of them: reversed-Z puts the nearer surface at the *larger* depth, so a
/// projection that inverted would give a scene whose occluders stand behind the
/// wall and occlude nothing — which draws a frame that is uniformly unoccluded
/// and passes anything asserted only on shape.
fn lifted_depth(projection: Mat4, lift: f32) -> f32 {
    let wall = depth_of(projection, -WALL_Z);
    let surface = depth_of(projection, -(WALL_Z - lift));
    assert!(
        surface > wall,
        "reversed-Z puts the nearer surface at the larger depth; a surface {lift} in front of the \
         wall reads {surface} and the wall {wall}, so this scene has it behind the wall and it \
         occludes nothing"
    );
    surface
}

/// The prepass image's contents, one depth per pixel of [`EXTENT`].
///
/// The caller hands over a function of the pixel rather than the scene taking a
/// shape, because the three tests want three different occluders out of
/// [`lifted_depth`]'s exact depths — a vertical strip for the radial line, a
/// tilted half-plane for the tangential one, and a strip against a strip for the
/// reconstruction. See this module's header on why the second cannot be the
/// first.
///
/// **At [`EXTENT`] whatever the pass reading it renders at.** This is the
/// prepass's image, and `ssao.slang` reads it at full resolution from a target
/// [`OCCLUSION_EXTENT`] wide — see that constant.
fn depth_image(depth: impl Fn(u32, u32) -> f32) -> Vec<u8> {
    let (width, height) = EXTENT;
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            bytes.extend_from_slice(&depth(x, y).to_le_bytes());
        }
    }
    bytes
}

/// The longest run of one value in `values`, as `(length, value)`.
fn longest_run(values: &[u8]) -> (usize, u8) {
    let mut best = (0, 0);
    let mut run = (0usize, 0u8);
    for &value in values {
        run = if value == run.1 {
            (run.0 + 1, value)
        } else {
            (1, value)
        };
        if run.0 > best.0 {
            best = run;
        }
    }
    best
}

/// Every plateau in `values`, as `value×length`, for the failure message.
fn plateaus(values: &[u8]) -> String {
    let mut out = Vec::new();
    for &value in values {
        match out.last_mut() {
            Some((last, count)) if *last == value => *count += 1,
            _ => out.push((value, 1usize)),
        }
    }
    out.iter()
        .map(|(value, count)| format!("{value}×{count}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The tilted occluder: the half-plane on the far side of an edge running along
/// [`EDGE_STEP`].
///
/// A predicate on two integers, so the edge is exact in the same sense the
/// upright plate's is — and it is a predicate on **depth-image** pixels, which
/// is what [`depth_image`] hands it.
fn occluded_diagonal(x: u32, y: u32) -> bool {
    across_edge(x, y) >= 0
}

/// How far `(x, y)` is from the tilted occluder's edge, in the edge's own units.
///
/// `EDGE_STEP.1 * x - EDGE_STEP.0 * y`, which is zero on the edge, positive
/// inside the occluder and — this is the property the whole measurement rests on
/// — **exactly unchanged by a step of [`EDGE_STEP`]**, since that step adds
/// `EDGE_STEP.1 * EDGE_STEP.0` and subtracts the same product. Divided by the
/// length of [`EDGE_STEP`] it is the perpendicular distance in pixels; the test
/// never needs that division except to check the line lies inside the falloff.
///
/// **It is linear, so it answers in the units of whichever grid it is given.**
/// A pixel of the gather stands for the depth texel
/// `crcbl_shaders::ssao::RESOLUTION_DIVISOR` times further out, so its distance
/// from the same edge is that many times smaller: the scene predicate above
/// calls this with depth-image pixels and [`DIAGONAL_OFFSET`] is stated in the
/// gather's. One edge, two grids, and an exact integer relation between them.
fn across_edge(x: u32, y: u32) -> i64 {
    EDGE_STEP.1 * i64::from(x) - EDGE_STEP.0 * i64::from(y)
}

/// The tangential measure: how many neighbouring pairs of `line` differ by more
/// than one level, and the largest difference any pair has.
///
/// Where [`longest_run`] is the radial one. A line whose true answer is a smooth
/// ramp has no runs worth counting and no pair worth calling a cliff — what a
/// slice count too small to cover the directions leaves is a step between one
/// tile phase and the next, and `docs/backlog.md`'s claim about the higher rung
/// is in exactly those terms: that it removes **every sharp edge**.
///
/// **The count is the number that carries the signal**, and the maximum is
/// reported beside it. An 8-bit channel holding a slowly-varying quantity steps
/// by one level wherever the quantiser rolls over, so one level is not an edge;
/// two is. See [`SHARP_EDGE`].
fn sharp_edges(line: &[u8]) -> (usize, u8) {
    let steps = line.windows(2).map(|pair| pair[0].abs_diff(pair[1]));
    steps.fold((0, 0), |(count, worst), step| {
        (count + usize::from(step >= SHARP_EDGE), worst.max(step))
    })
}

/// The two bind-group layouts and the three pipelines, built exactly as
/// `crates/crcbl-render/src/ssao.rs` builds them.
///
/// **Two layouts for three pipelines**, which is that module's shape and not a
/// saving made here: `ssao_upsample.slang` declares the blur's three bindings in
/// the blur's order — the block, an `R8Unorm` occlusion image, the scene's depth
/// — so one layout describes both and a second would be one more description to
/// keep in step.
struct Passes {
    layout: crcbl::hal::BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    blur_layout: crcbl::hal::BindGroupLayoutHandle,
    blur_pipeline_layout: PipelineLayoutHandle,
    blur_pipeline: GraphicsPipelineHandle,
    /// The reconstruction, built whether or not this run's chain ends with it —
    /// one shape rather than two, and nothing downstream can bind a pipeline it
    /// was not handed.
    upsample_pipeline: GraphicsPipelineHandle,
}

/// A full-screen-triangle pipeline over `shader`, which is
/// `ForwardRenderer::build_fullscreen`'s shape: no depth state, no
/// multisampling, one colour target, and the module released before the result
/// is unwrapped.
fn fullscreen_pipeline(
    device: &dyn Device,
    label: &str,
    shader: &Shader,
    layout: PipelineLayoutHandle,
) -> GraphicsPipelineHandle {
    let vertex = shader
        .entry_point(Stage::Vertex)
        .expect("a vertex entry point");
    let fragment = shader
        .entry_point(Stage::Fragment)
        .expect("a fragment entry point");
    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some(label),
            spirv: shader.spirv(),
            wgsl: shader.wgsl(),
            msl: shader.msl(),
            dxil: &shader.dxil_containers(),
        })
        .expect("a shader module");
    let targets = [ColorTargetState::opaque(Format::R8Unorm)];
    let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
        label: Some(label),
        layout,
        vertex: ShaderEntry {
            module,
            entry_point: vertex,
        },
        fragment: Some(ShaderEntry {
            module,
            entry_point: fragment,
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        color_targets: &targets,
    });
    device.destroy_shader_module(module);
    pipeline.expect("a full-screen pipeline")
}

impl Passes {
    /// Both layouts and both pipelines. The entries are
    /// `crates/crcbl-render/src/ssao.rs`'s, including the `Depth` sample type
    /// that `DepthTexture2D` needs on WebGPU.
    fn new(device: &dyn Device) -> Self {
        let uniform = BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::UniformBuffer { dynamic: false },
            count: 1,
            flags: BindingFlags::empty(),
        };
        let sampled = |binding: u32, sample_type: SampleType| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type,
            },
            count: 1,
            flags: BindingFlags::empty(),
        };

        let entries = [uniform, sampled(1, SampleType::Depth)];
        let layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("ssao depth"),
                entries: &entries,
            })
            .expect("the occlusion layout");
        let set_layouts = [layout];
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("ssao"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("the occlusion pipeline layout");

        let blur_entries = [
            uniform,
            sampled(1, SampleType::Float),
            sampled(2, SampleType::Depth),
        ];
        let blur_layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("ssao blur"),
                entries: &blur_entries,
            })
            .expect("the blur layout");
        let blur_set_layouts = [blur_layout];
        let blur_pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("ssao blur"),
                bind_group_layouts: &blur_set_layouts,
                push_constants: None,
            })
            .expect("the blur pipeline layout");

        Self {
            pipeline: fullscreen_pipeline(device, "ssao", &SSAO, pipeline_layout),
            blur_pipeline: fullscreen_pipeline(
                device,
                "ssao blur",
                &SSAO_BLUR,
                blur_pipeline_layout,
            ),
            upsample_pipeline: fullscreen_pipeline(
                device,
                "ssao upsample",
                &SSAO_UPSAMPLE,
                blur_pipeline_layout,
            ),
            layout,
            pipeline_layout,
            blur_layout,
            blur_pipeline_layout,
        }
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_graphics_pipeline(self.upsample_pipeline);
        device.destroy_graphics_pipeline(self.blur_pipeline);
        device.destroy_pipeline_layout(self.blur_pipeline_layout);
        device.destroy_bind_group_layout(self.blur_layout);
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
    }
}

/// One `R8Unorm` target of `extent`, sampled by the pass after it.
///
/// The extent is a parameter because the chain has two: the gather and the blur
/// write [`OCCLUSION_EXTENT`] and the reconstruction writes [`EXTENT`].
fn occlusion_image(
    device: &dyn Device,
    label: &str,
    extent: (u32, u32),
) -> (ImageHandle, ImageViewHandle) {
    let image = device
        .create_image(&ImageDesc {
            label: Some(label),
            image_type: ImageType::D2,
            extent: Extent3d::d2(extent.0, extent.1),
            format: Format::R8Unorm,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED | ImageUsage::TRANSFER_SRC,
        })
        .expect("an occlusion target");
    let view = device
        .create_image_view(&ImageViewDesc {
            label: Some(label),
            image,
            view_type: ImageViewType::D2,
            format: Format::R8Unorm,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 0,
                mip_count: 1,
                base_layer: 0,
                layer_count: 1,
            },
        })
        .expect("an occlusion view");
    (image, view)
}

/// The whole colour range of a single-level, single-layer image.
fn colour_range() -> ImageSubresourceRange {
    ImageSubresourceRange {
        aspect: ImageAspect::COLOR,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    }
}

/// One run of the shipping chain over an analytic prepass image.
///
/// The whole final channel, and what each pass cost — the two things every test
/// here wants and none can get from another's readback.
struct Run {
    /// The last pass's occlusion, one byte per pixel and [`Run::extent`]`.0`
    /// bytes to a row. Whole rather than the one line a test reads, because the
    /// tangential line is a diagonal and a diagonal is not a copy region.
    image: Vec<u8>,
    /// The extent [`Run::image`] was read back at, which is the extent of
    /// whichever pass ended the chain — see [`Chain`].
    extent: (u32, u32),
    /// `(label, nanoseconds)` per pass in the order they ran, or empty on a
    /// device with no timestamp query.
    ///
    /// **Every pass is bracketed separately**, unlike the single bracket this
    /// suite carried when there was one variable: the slice count is paid for
    /// inside `ssao` and a second blur is a whole extra full-screen read and
    /// write, and a reader deciding whether a tier can afford the rung needs the
    /// two prices apart. See `crcbl_render::ssao`'s header.
    timings: Vec<(String, u64)>,
}

impl Run {
    /// This run's value at `(x, y)`, in the pixels of the pass that produced it.
    fn at(&self, x: u32, y: u32) -> u8 {
        self.image[(y * self.extent.0 + x) as usize]
    }

    /// What one pass cost, by label.
    fn pass(&self, label: &str) -> Option<u64> {
        self.timings
            .iter()
            .find(|(name, _)| name == label)
            .map(|(_, nanos)| *nanos)
    }
}

/// One configuration of the two switches, and what the tangential line measured
/// under it.
///
/// A type rather than a tuple because the sweep looks its entries up by the pair
/// of counts and then reads two statistics off them, and `.2` against `.3` is
/// exactly the pair a reader would mix up.
struct Measured {
    slices: u8,
    blurs: u32,
    /// The line's samples, kept for the failure message.
    line: Vec<u8>,
    /// Neighbouring pairs differing by [`SHARP_EDGE`] or more — see
    /// [`sharp_edges`].
    sharp: usize,
    /// The largest difference any neighbouring pair had.
    worst: u8,
}

/// Where a run's chain ends, and therefore what extent its readback is in.
///
/// **Two chains rather than one**, because the two questions this file asks want
/// different halves of the pass list. The gradients are about `ssao.slang`'s own
/// dither ladder and `ssao_upsample.slang` is a bilateral filter over exactly
/// that ladder, so reading them through it would measure the smoothing; the
/// reconstruction is about what that filter does at a silhouette, which it can
/// only be asked at the extent it writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chain {
    /// The gather and its blurs, read back at [`OCCLUSION_EXTENT`].
    Gathered,
    /// The whole shipping chain including `ssao_upsample.slang`, read back at
    /// [`EXTENT`].
    Reconstructed,
}

/// Runs `ssao`, `blurs` blur passes and — when `chain` asks for it — the
/// reconstruction over `texels`, at `slices` planes per pixel, and reads the
/// last pass's output back whole.
///
/// The pipelines and both layouts are `crates/crcbl-render/src/ssao.rs`'s, and
/// the second blur is that module's `ssao-blur-2`: the same pipeline reading the
/// first one's output into a third image. Nothing here reaches into that crate —
/// `Ssao` is private to it — so the shape is copied and this comment is what
/// says where from.
///
/// **Three extents pass through here.** `texels` fills the depth image at
/// [`EXTENT`]; the gather and every blur render into [`OCCLUSION_EXTENT`]; the
/// reconstruction renders into [`EXTENT`] again. Each pass's render area is its
/// own target's, which is what `crcbl_render::ssao::Ssao::add_passes` gets from
/// the graph and what this has to spell for itself.
fn run_passes(
    headless: &Headless,
    projection: Mat4,
    texels: &[u8],
    slices: u8,
    blurs: u32,
    intensity: f32,
    chain: Chain,
) -> Run {
    let device = headless.device.as_ref();
    let (width, height) = EXTENT;
    let (gather_width, gather_height) = OCCLUSION_EXTENT;

    // **The prepass image is filled by a copy, and not every backend can.**
    // WebGPU defines a buffer-to-image copy for `D16Unorm`'s depth plane alone
    // — `crcbl::hal::Capability::DepthImageCopy` carries the table — so a
    // backend that cannot do it would otherwise fail somewhere inside the
    // submission with nothing naming the reason. No such backend runs this
    // suite today; this is what says so if one arrives.
    assert!(
        device.supports(Capability::DepthImageCopy).is_yes(),
        "this backend cannot copy a buffer into a depth image, so the analytic prepass this test \
         measures from cannot be built on it. See `crcbl::hal::Capability::DepthImageCopy` for \
         which formats each API moves and in which direction."
    );

    let passes = Passes::new(device);
    let (raw, raw_view) = occlusion_image(device, "ssao", OCCLUSION_EXTENT);
    let (blurred, blurred_view) = occlusion_image(device, "ssao-blurred", OCCLUSION_EXTENT);
    // The second blur's target, created only when one was asked for, on the
    // renderer's terms: a transient nothing reads or writes is an image taken
    // out for a pass that does not exist.
    let second = (blurs > 1).then(|| occlusion_image(device, "ssao-blurred-2", OCCLUSION_EXTENT));
    // The reconstruction's, on the same terms and at the scene's own extent —
    // see [`Chain`].
    let upsampled =
        (chain == Chain::Reconstructed).then(|| occlusion_image(device, "ssao-upsampled", EXTENT));

    // The prepass image, filled from the host. `SAMPLED` because every pass
    // reads it, `TRANSFER_DST` because this is where it comes from.
    let depth = device
        .create_image(&ImageDesc {
            label: Some("scene-depth"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(width, height),
            format: Format::D32Float,
            mip_levels: 1,
            samples: 1,
            // `crcbl_render::TransientImageDesc::scene_depth`'s pair, plus the
            // transfer this fills it through. The attachment usage is carried
            // even though nothing renders into it here: it is what the shipping
            // image is created with, and an image created with a different
            // usage is an image a driver may lay out differently.
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT
                | ImageUsage::SAMPLED
                | ImageUsage::TRANSFER_DST,
        })
        .expect("a prepass image");
    let depth_range = ImageSubresourceRange {
        aspect: ImageAspect::DEPTH,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    };
    let depth_view = device
        .create_image_view(&ImageViewDesc {
            label: Some("scene-depth"),
            image: depth,
            view_type: ImageViewType::D2,
            format: Format::D32Float,
            range: depth_range,
        })
        .expect("a prepass view");

    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("scene-depth upload"),
            size: texels.len() as u64,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("an upload buffer");
    device.write_buffer(upload, 0, texels).expect("the upload");

    let params = SsaoParams {
        inv_proj: projection.inverse().to_cols_array(),
        proj: projection.to_cols_array(),
        radius: RADIUS,
        slices,
        intensity,
    };
    let uniforms = device
        .create_buffer(&BufferDesc {
            label: Some("ssao params"),
            size: crcbl::shaders::ssao::PARAMS_SIZE as u64,
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        })
        .expect("the uniform block");
    device
        .write_buffer(uniforms, 0, &params.to_bytes())
        .expect("the block is written");

    let group = |label: &str, layout, entries: Vec<BindGroupEntry>| -> BindGroupHandle {
        device
            .create_bind_group(&BindGroupDesc {
                label: Some(label),
                layout,
                entries: &entries,
                variable_count: None,
            })
            .expect("a bind group")
    };
    let occlusion_group = group(
        "ssao depth",
        passes.layout,
        vec![
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::whole_buffer(uniforms),
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::ImageView(depth_view),
            },
        ],
    );
    // The blur's three bindings, which are also the reconstruction's three — see
    // [`Passes`] on why one layout answers for both.
    let filter_group = |source: ImageViewHandle| {
        group(
            "ssao blur",
            passes.blur_layout,
            vec![
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(uniforms),
                },
                BindGroupEntry {
                    binding: 1,
                    array_index: 0,
                    resource: BindingResource::ImageView(source),
                },
                BindGroupEntry {
                    binding: 2,
                    array_index: 0,
                    resource: BindingResource::ImageView(depth_view),
                },
            ],
        )
    };
    // Every blur's target in order: the first one's image, and the second one's
    // where a second was asked for.
    let blur_targets: Vec<(ImageHandle, ImageViewHandle)> =
        core::iter::once((blurred, blurred_view))
            .chain(second)
            .collect();
    // One group per blur, each naming the image *before* it: the raw channel for
    // the first and the previous blur's target after that.
    let blur_groups: Vec<BindGroupHandle> = core::iter::once(raw_view)
        .chain(blur_targets.iter().map(|(_, view)| *view))
        .take(blurs as usize)
        .map(filter_group)
        .collect();
    assert_eq!(
        blur_groups.len(),
        blurs as usize,
        "one group per blur, or a pass below binds the group of the pass before it"
    );
    // The reconstruction reads the last blur's target, which is the image the
    // chain would otherwise have been read back from.
    let last_blur = blur_targets[blurs as usize - 1];
    let upsample_group = upsampled.map(|_| filter_group(last_blur.1));

    // What the readback is in: the reconstruction's extent when it ran and the
    // pair's otherwise — see [`Chain`].
    let (read_width, read_height) = match chain {
        Chain::Gathered => OCCLUSION_EXTENT,
        Chain::Reconstructed => EXTENT,
    };

    // The whole channel, not a row: the tangential line is a diagonal, and a
    // copy region cannot be one. At [`EXTENT`] this is two megabytes, which is
    // one readback on a suite that already uploads eight.
    let alignment = device
        .caps()
        .limits
        .optimal_buffer_copy_offset_alignment
        .max(4);
    let pitch = u64::from(read_width).next_multiple_of(alignment);
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("ssao image"),
            size: pitch * u64::from(read_height),
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    // Two queries per pass, in the order the passes run — see [`Run::timings`]
    // on why every pass is bracketed rather than the pair.
    let labels: Vec<String> = core::iter::once("ssao".to_owned())
        .chain((0..blurs).map(|index| match index {
            0 => "ssao-blur".to_owned(),
            other => format!("ssao-blur-{}", other + 1),
        }))
        .chain(upsampled.map(|_| "ssao-upsample".to_owned()))
        .collect();
    let queries = u32::try_from(labels.len() * 2).expect("a handful of passes");
    let timestamps = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let timers = timestamps.then(|| {
        device
            .create_query_set(&QuerySetDesc {
                label: Some("ssao"),
                kind: QueryKind::Timestamp,
                count: queries,
            })
            .expect("a timestamp pair per pass on a device that reports the feature")
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ssao measurement"),
        queue: headless.queue,
    });
    if let Some(set) = timers {
        encoder.reset_query_set(set, 0..queries);
    }
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            depth,
            depth_range,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_image(&BufferImageCopy {
        buffer: upload,
        buffer_offset: 0,
        buffer_row_length: width,
        buffer_image_height: height,
        image: depth,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::DEPTH,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[
            ImageBarrier::new(
                depth,
                depth_range,
                ResourceState::TransferDst,
                ResourceState::ShaderRead,
            ),
            ImageBarrier::new(
                raw,
                colour_range(),
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            ),
        ],
        ..Barriers::default()
    });

    // The gather's and every blur's, which is not the reconstruction's — see
    // this function's header on the three extents.
    let area = Rect2d::from_size(gather_width, gather_height);
    let viewport = Viewport::from_size(gather_width, gather_height);
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("ssao"),
        color_attachments: &[ColorAttachment {
            view: raw_view,
            resolve: None,
            load: LoadOp::DontCare,
            store: StoreOp::Store,
            clear: ClearValue::default(),
        }],
        depth_stencil_attachment: None,
        render_area: area,
        timestamp_writes: timers.map(|set| PassTimestampWrites {
            set,
            beginning_of_pass: 0,
            end_of_pass: 1,
        }),
    });
    encoder.set_viewport(&viewport);
    encoder.set_scissor(&area);
    encoder.bind_graphics_pipeline(passes.pipeline);
    encoder.bind_group(0, occlusion_group, &[], passes.pipeline_layout);
    encoder.draw(0..3, 0..1);
    encoder.end_render_pass();

    // Each blur reads the image before it and writes the next one. `source` is
    // the raw channel for the first and the previous blur's target after that,
    // which is the ping-pong `crcbl_render::ssao::Ssao::add_passes` records.
    let mut source = (raw, raw_view);
    for (index, &target) in blur_targets.iter().take(blurs as usize).enumerate() {
        encoder.pipeline_barrier(&Barriers {
            images: &[
                ImageBarrier::new(
                    source.0,
                    colour_range(),
                    ResourceState::ColorAttachment,
                    ResourceState::ShaderRead,
                ),
                ImageBarrier::new(
                    target.0,
                    colour_range(),
                    ResourceState::Undefined,
                    ResourceState::ColorAttachment,
                ),
            ],
            ..Barriers::default()
        });
        let first = u32::try_from(index * 2 + 2).expect("a handful of passes");
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("ssao-blur"),
            color_attachments: &[ColorAttachment {
                view: target.1,
                resolve: None,
                load: LoadOp::DontCare,
                store: StoreOp::Store,
                clear: ClearValue::default(),
            }],
            depth_stencil_attachment: None,
            render_area: area,
            timestamp_writes: timers.map(|set| PassTimestampWrites {
                set,
                beginning_of_pass: first,
                end_of_pass: first + 1,
            }),
        });
        encoder.set_viewport(&viewport);
        encoder.set_scissor(&area);
        encoder.bind_graphics_pipeline(passes.blur_pipeline);
        encoder.bind_group(0, blur_groups[index], &[], passes.blur_pipeline_layout);
        encoder.draw(0..3, 0..1);
        encoder.end_render_pass();

        source = target;
    }

    // The reconstruction, at the scene's own extent and over the last blur's
    // output — `crcbl_render::ssao::Ssao::add_passes`' last pass, and the one
    // whose target the forward pass binds.
    if let (Some(target), Some(group)) = (upsampled, upsample_group) {
        encoder.pipeline_barrier(&Barriers {
            images: &[
                ImageBarrier::new(
                    source.0,
                    colour_range(),
                    ResourceState::ColorAttachment,
                    ResourceState::ShaderRead,
                ),
                ImageBarrier::new(
                    target.0,
                    colour_range(),
                    ResourceState::Undefined,
                    ResourceState::ColorAttachment,
                ),
            ],
            ..Barriers::default()
        });
        let first = blurs * 2 + 2;
        let scene = Rect2d::from_size(width, height);
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("ssao-upsample"),
            color_attachments: &[ColorAttachment {
                view: target.1,
                resolve: None,
                load: LoadOp::DontCare,
                store: StoreOp::Store,
                clear: ClearValue::default(),
            }],
            depth_stencil_attachment: None,
            render_area: scene,
            timestamp_writes: timers.map(|set| PassTimestampWrites {
                set,
                beginning_of_pass: first,
                end_of_pass: first + 1,
            }),
        });
        encoder.set_viewport(&Viewport::from_size(width, height));
        encoder.set_scissor(&scene);
        encoder.bind_graphics_pipeline(passes.upsample_pipeline);
        encoder.bind_group(0, group, &[], passes.blur_pipeline_layout);
        encoder.draw(0..3, 0..1);
        encoder.end_render_pass();

        source = target;
    }

    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            source.0,
            colour_range(),
            ResourceState::ColorAttachment,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: u32::try_from(pitch).expect("a row of a 1080p frame"),
        buffer_image_height: read_height,
        image: source.0,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d::default(),
        image_extent: Extent3d::d2(read_width, read_height),
    });

    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    let mut padded = poisoned((pitch * u64::from(read_height)) as usize);
    headless.readback(staging, pitch * u64::from(read_height), &mut padded);
    let mut image = Vec::with_capacity((read_width * read_height) as usize);
    for row in 0..read_height {
        let at = (u64::from(row) * pitch) as usize;
        image.extend_from_slice(&padded[at..at + read_width as usize]);
    }

    let timings = match timers {
        Some(set) => {
            let mut readings = vec![0u64; queries as usize];
            device
                .query_results(set, 0, &mut readings)
                .expect("the timestamps resolve");
            labels
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    (
                        label.clone(),
                        readings[index * 2 + 1].saturating_sub(readings[index * 2]),
                    )
                })
                .collect()
        }
        None => Vec::new(),
    };

    if let Some(set) = timers {
        device.destroy_query_set(set);
    }
    if let Some(group) = upsample_group {
        device.destroy_bind_group(group);
    }
    for group in blur_groups {
        device.destroy_bind_group(group);
    }
    device.destroy_bind_group(occlusion_group);
    device.destroy_buffer(staging);
    device.destroy_buffer(uniforms);
    device.destroy_buffer(upload);
    device.destroy_image_view(depth_view);
    device.destroy_image(depth);
    if let Some((image, view)) = upsampled {
        device.destroy_image_view(view);
        device.destroy_image(image);
    }
    if let Some((image, view)) = second {
        device.destroy_image_view(view);
        device.destroy_image(image);
    }
    device.destroy_image_view(blurred_view);
    device.destroy_image(blurred);
    device.destroy_image_view(raw_view);
    device.destroy_image(raw);
    passes.destroy(device);

    Run {
        image,
        extent: (read_width, read_height),
        timings,
    }
}

/// **The blurred occlusion falls off smoothly, and a march that started every
/// pixel at the same fraction of a step made it a staircase.**
///
/// A wall with a plate in front of it, both written into the prepass image as
/// exact depths, through the shipping `ssao` and `ssao-blur` pipelines at 1080p.
/// The row through the plate's edge is read back and the longest run of one
/// 8-bit level inside the falloff is measured: the baseline shape is runs of a
/// step's width, which `ssao.slang`'s `STEP_OFFSETS` breaks into runs a blur
/// footprint wide.
///
/// **At the counts that ship**, which is the point of it: this is the radial
/// artefact and the slice count is not what fixes it. The tangential test below
/// is where the higher rung is measured.
///
/// The failure message carries the whole plateau list, because the number that
/// went red says the window terraced and only the list says *how*.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_blurred_occlusion_falloff_does_not_terrace() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
    let (width, height) = EXTENT;
    let size = (width as f32, height as f32);
    let projection = Projection::Perspective {
        fov_y: FOV_Y,
        near: NEAR,
    }
    .matrix(size.0 / size.1);

    // The falloff cannot be longer than the radius it is measured within, so a
    // scene whose radius no longer covers the window is one where the window
    // runs off the end into flat unoccluded wall. The assertion on the window's
    // own last column catches that too; this catches it with the scene named.
    //
    // **Two numbers for one radius, in the two grids this chain has.** `reach`
    // is `occlusion_at`'s own — the projected radius in depth-image pixels,
    // which is what the march steps in — and `covered` is the same length in
    // the gather's pixels, which is what the window is counted in. See
    // [`OCCLUSION_EXTENT`].
    let reach = reach_in_pixels(projection, size);
    let covered = reach / crcbl::shaders::ssao::RESOLUTION_DIVISOR as f32;
    let needed = f32::from(u16::try_from(SILHOUETTE_SKIP + WINDOW).expect("a window under a row"));
    assert!(
        covered >= needed,
        "the scene projects a {RADIUS} radius to {covered:.1} of the gather's pixels at a wall \
         {WALL_Z} away, and the window past the silhouette is {needed} columns wide — so it \
         cannot fit inside the falloff. Move the wall closer, widen the field of view, or shorten \
         the window."
    );

    let wall = depth_of(projection, -WALL_Z);
    let plate = lifted_depth(projection, PLATE_LIFT);
    let texels = depth_image(|x, _| {
        if PLATE_COLUMNS.contains(&x) {
            plate
        } else {
            wall
        }
    });
    let run = run_passes(
        &headless,
        projection,
        &texels,
        crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
        1,
        crcbl::shaders::ssao::INTENSITY_DEFAULT,
        Chain::Gathered,
    );

    // The plate's edge in the gather's own pixels: `full_res_pixel` run
    // backwards, rounded **up** so this is the first pixel of the pass whose
    // texel is off the plate rather than the last one still on it.
    let edge = PLATE_COLUMNS
        .end
        .div_ceil(crcbl::shaders::ssao::RESOLUTION_DIVISOR);
    let start = edge + SILHOUETTE_SKIP;
    let row: Vec<u8> = (0..WINDOW)
        .map(|column| run.at(start + column, LINE_Y))
        .collect();
    let window = row.as_slice();
    let (run_length, level) = longest_run(window);
    let low = *window.iter().min().expect("a non-empty window");
    let high = *window.iter().max().expect("a non-empty window");
    let shape = plateaus(window);

    match run.pass("ssao") {
        Some(ns) => eprintln!(
            "{suite}: the ssao pass took {ns} ns over a {width}×{height} scene gathered at \
             {gather}×{rows}; the falloff's longest run is {run_length} columns at level {level} \
             over {low}..={high}",
            suite = crate::SUITE,
            gather = OCCLUSION_EXTENT.0,
            rows = OCCLUSION_EXTENT.1,
        ),
        None => eprintln!(
            "{suite}: this device has no timestamp query, so the ssao pass is untimed here; the \
             falloff's longest run is {run_length} columns at level {level} over {low}..={high}",
            suite = crate::SUITE,
        ),
    }

    assert!(
        high < UNOCCLUDED,
        "the {WINDOW}-column window past the plate's edge reaches {UNOCCLUDED:#04x}, so the \
         falloff ended before the window did and the run below is measured over flat unoccluded \
         wall. The window is {shape}. Move the window or the wall — the projected radius is \
         {covered:.1} of the gather's pixels."
    );
    assert!(
        high - low >= MIN_SWING,
        "the window spans {low}..={high}, a swing of {}, and this measurement needs at least \
         {MIN_SWING} levels of gradient to have anything to say about its shape. The window is \
         {shape}.",
        high - low
    );
    assert!(
        run_length < MAX_RUN,
        "the blurred occlusion terraces: {run_length} consecutive columns at level {level} inside \
         the {WINDOW}-column falloff past the plate's edge, where {MAX_RUN} is the bound. The \
         whole window is {shape}.\n\
         \x20 What this measured against: `ssao.slang` with every `STEP_OFFSETS` entry forced to \
         one — the march that starts every pixel at the same fraction of a step — gave a longest \
         run of 12 columns on radv and on lavapipe alike, against 4 with the table in place. A \
         step of the march is {step:.1} depth-image pixels ({reach:.1}-pixel reach over \
         {SLICE_STEPS_HINT} steps), which is {covered_step:.1} of the gather's own — so a run of \
         several columns is the horizon landing on {SLICE_STEPS_HINT} distances with nothing \
         dithering them across the blur's footprint.",
        step = reach / SLICE_STEPS_HINT as f32,
        covered_step = covered / SLICE_STEPS_HINT as f32,
    );

    headless.finish();
}

/// **A line parallel to a tilted occluder's edge should be one constant, and
/// the slice count is what decides how far it is from one.**
///
/// The scene is [`the_blurred_occlusion_falloff_does_not_terrace`]'s two depths
/// with the plate tilted to the diagonal — see this module's header on why the
/// upright plate cannot show this and why the diagonal costs no rounding. Every
/// sample on the line sits at the same perpendicular distance from the edge, so
/// the continuous answer is flat and every level of variation is the pass's own
/// tile showing through the blur.
///
/// # What is asserted, and what is only reported
///
/// Two things, and both are about the shipping configuration being *no worse
/// than it is*: the line's largest step between neighbours, and its
/// peak-to-peak. Neither is a claim that the rung is affordable — that is a
/// tier's decision and lives in `docs/backlog.md` — so the four configurations
/// are timed and printed rather than compared for speed.
///
/// The third assertion is the one the rung rests on: **four slices and two
/// blurs must measure flatter than two and one.** A change that made the extra
/// planes stop contributing — an eighth turn that landed back on the quarter
/// turn, a count the shader clamped away — leaves every other check here green.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_tangential_occlusion_line_does_not_step() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
    let (width, height) = EXTENT;
    let size = (width as f32, height as f32);
    let projection = Projection::Perspective {
        fov_y: FOV_Y,
        near: NEAR,
    }
    .matrix(size.0 / size.1);

    // In the gather's pixels, which is the grid the line is read in and the grid
    // [`DIAGONAL_OFFSET`] is stated in — see [`OCCLUSION_EXTENT`] and the note
    // beside the falloff test's own reach.
    let reach = reach_in_pixels(projection, size) / crcbl::shaders::ssao::RESOLUTION_DIVISOR as f32;
    // The line's perpendicular distance from the edge, which is the one place
    // this test leaves the edge's own integer units. It has to sit inside the
    // falloff, which is as deep as the projected radius and no deeper — named
    // here rather than left to the assertions below, which would go red on flat
    // unoccluded wall for a reason that is not theirs.
    let edge_length = ((EDGE_STEP.0 * EDGE_STEP.0 + EDGE_STEP.1 * EDGE_STEP.1) as f64).sqrt();
    let lift = DIAGONAL_OFFSET as f64 / edge_length;
    assert!(
        f64::from(reach) > lift,
        "the scene projects a {RADIUS} radius to {reach:.1} of the gather's pixels and the line \
         sits {lift:.1} of them from the edge — outside the falloff, where the occlusion is flat because there is \
         none rather than because the pass is smooth"
    );
    assert_eq!(
        across_edge(DIAGONAL_FROM.0, DIAGONAL_FROM.1),
        -DIAGONAL_OFFSET,
        "the line's first pixel does not sit `DIAGONAL_OFFSET` from the edge, so the distance the \
         guard above checked is not the distance the samples were taken at"
    );

    let wall = depth_of(projection, -WALL_Z);
    let plate = lifted_depth(projection, PLATE_LIFT);
    let texels = depth_image(|x, y| if occluded_diagonal(x, y) { plate } else { wall });

    // **Warm up, and throw the answer away.** Whichever configuration runs first
    // in a process pays for both pipelines' creation, which on a software
    // rasteriser is the shader's own compilation and is an order of magnitude
    // more than the draw — measured on lavapipe 2026-08-31, the first `ssao`
    // reading of a process was 17 ms against 12 ms for the same work later in
    // the same process. Nothing is asserted on the timings, but a printed number
    // that is mostly a compiler is a number someone will quote.
    let _ = run_passes(
        &headless,
        projection,
        &texels,
        crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
        1,
        crcbl::shaders::ssao::INTENSITY_DEFAULT,
        Chain::Gathered,
    );

    // Every combination of the two switches, so the two purchases can be priced
    // apart — see `crcbl_render::ssao`'s header on why they are two variables.
    // The counts are the shader's own constants rather than literals, so a
    // sweep that stopped covering the range would be a compile error.
    let low = crcbl::shaders::ssao::SLICE_COUNT_DEFAULT;
    let high = crcbl::shaders::ssao::SLICE_COUNT_MAX;
    let mut lines = Vec::new();
    for (slices, blurs) in [(low, 1u32), (high, 1), (low, 2), (high, 2)] {
        let run = run_passes(
            &headless,
            projection,
            &texels,
            slices,
            blurs,
            crcbl::shaders::ssao::INTENSITY_DEFAULT,
            Chain::Gathered,
        );
        let line: Vec<u8> = (0..DIAGONAL_LENGTH)
            .map(|step| {
                let along = u32::try_from(EDGE_STEP.0).expect("a step of a few pixels") * step;
                let down = u32::try_from(EDGE_STEP.1).expect("a step of a few pixels") * step;
                run.at(DIAGONAL_FROM.0 + along, DIAGONAL_FROM.1 + down)
            })
            .collect();
        let (sharp, worst) = sharp_edges(&line);
        let low = *line.iter().min().expect("a non-empty line");
        let high = *line.iter().max().expect("a non-empty line");
        let march = run.pass("ssao");
        let blur = run.pass("ssao-blur");
        let second = run.pass("ssao-blur-2");
        eprintln!(
            "{suite}: {slices} slices, {blurs} blur: the tangential line has {sharp} sharp edges \
             in {DIAGONAL_LENGTH} samples, worst step {worst}, over {low}..={high}; ssao \
             {march:?} ns, ssao-blur {blur:?} ns, ssao-blur-2 {second:?} ns",
            suite = crate::SUITE,
        );
        lines.push(Measured {
            slices,
            blurs,
            line,
            sharp,
            worst,
        });
    }

    let at = |slices: u8, blurs: u32| {
        lines
            .iter()
            .find(|measured| measured.slices == slices && measured.blurs == blurs)
            .unwrap_or_else(|| panic!("{slices} slices and {blurs} blur(s) was measured"))
    };
    let shipped = at(low, 1);
    let rung = at(high, 2);
    // The rung's own blur count with the shipping slice count, which is what
    // isolates the eighth turn: comparing the rung against what ships would let
    // the second blur alone answer for it.
    let blurred_twice = at(low, 2);

    // Anti-vacuity, on `MIN_SWING`'s terms: a line lying outside the falloff, or
    // on a scene whose plate occludes nothing, is flat for a reason that has
    // nothing to do with the pass and would pass every assertion below.
    let level = shipped.line[0];
    assert!(
        level < UNOCCLUDED && level > 0,
        "the tangential line reads {level} at its first sample, which is neither occluded nor \
         lit — the line is outside the falloff or the plate is not in front of the wall, and a \
         flatness measured there says nothing about the pass"
    );

    assert!(
        shipped.sharp <= MAX_SHARP_EDGES,
        "the tangential line has {} sharp edges in {DIAGONAL_LENGTH} samples at the counts that \
         ship, where {MAX_SHARP_EDGES} is the bound and its worst step is {} levels. The line is \
         a straight run at a fixed distance from a straight edge, so what is stepping is \
         `ssao.slang`'s tile coming through `ssao_blur.slang` rather than anything in the scene. \
         Its first samples are {first:?}.",
        shipped.sharp,
        shipped.worst,
        first = &shipped.line[..MESSAGE_SAMPLES.min(shipped.line.len())],
    );
    assert!(
        rung.sharp < blurred_twice.sharp,
        "the eighth turn bought nothing: {high} slices left {} sharp edges against {low} slices' \
         {}, at the same two blur passes. Either `ssao.slang`'s eighth turn landed back on a \
         direction the quarter turn already gave, or the count never reached the shader — and \
         every other check here is green either way. This is the one assertion about the rung \
         itself.",
        rung.sharp,
        blurred_twice.sharp,
    );

    headless.finish();
}

/// The halo measurement, and the two assertions it is: what the bar reads
/// against what the wall behind it reads.
///
/// **One function because it is one measurement.** The pair of tests below
/// differ in the scene they build and the line they read it along — a bar
/// spanning every row, read across a row, and the same bar turned a quarter
/// turn, read down a column — and in nothing after that. A halo is the
/// background's occlusion arriving on the foreground, which is the bar's
/// brightest sample less its darkest whichever way the silhouette runs, and
/// [`MAX_HALO`] and [`MIN_HALO_CONTRAST`] are arguments about
/// `ssao_upsample.slang` rather than about an axis: a change to either has to
/// move both tests, so both read them from here.
///
/// `across` names what `span` counts — `"columns"` or `"rows"` — so the line
/// this prints and the message it fails with say which of the pair spoke.
///
/// **What it cannot see is which axis it was handed.** It is given two lines of
/// samples and knows nothing about where they came from, so a scene that read
/// the wrong line, or the same line twice, is not something this can catch —
/// that is the callers' territory, and each of them asserts its own placement.
fn assert_no_halo(
    run: &Run,
    across: &str,
    span: core::ops::Range<u32>,
    front: &[u8],
    behind: &[u8],
) {
    let (width, height) = EXTENT;
    let lit = *front.iter().max().expect("a non-empty bar");
    let dip = lit - *front.iter().min().expect("a non-empty bar");
    let background = *behind.iter().min().expect("a non-empty wall margin");
    let contrast = lit.saturating_sub(background);

    match run.pass("ssao-upsample") {
        Some(ns) => eprintln!(
            "{suite}: the ssao-upsample pass took {ns} ns at {width}×{height}; the foreground bar \
             across {across} {span:?} reads {lit} at its brightest and dips {dip} against a wall \
             at {background}",
            suite = crate::SUITE,
        ),
        None => eprintln!(
            "{suite}: this device has no timestamp query, so the ssao-upsample pass is untimed \
             here; the foreground bar across {across} {span:?} reads {lit} at its brightest and \
             dips {dip} against a wall at {background}",
            suite = crate::SUITE,
        ),
    }

    // Anti-vacuity, on [`MIN_SWING`]'s terms: a halo is the difference between
    // two surfaces, so a scene whose wall is as bright as its bar has nothing to
    // draw one out of and would pass the assertion below however the taps were
    // weighted.
    assert!(
        contrast >= MIN_HALO_CONTRAST,
        "the bar reads {lit} and the wall behind it {background}, a contrast of {contrast} — under \
         the {MIN_HALO_CONTRAST} levels this measurement needs to have anything to say. The wall \
         beside the bar is not occluded enough to halo with: move the bar further inside the \
         plate's falloff, or the plate closer to the wall."
    );
    assert!(
        dip <= MAX_HALO,
        "the reconstruction haloes: the foreground bar dips {dip} levels below its own {lit} \
         inside {across} {span:?}, where {MAX_HALO} is the bound, against a wall reading \
         {background}. The bar is a flat surface with nothing within {RADIUS} of it, so every \
         pixel of it is unoccluded and the dip is the wall's occlusion arriving through \
         `ssao_upsample.slang` — its depth weight has stopped rejecting a tap across the \
         silhouette. The bar's first {across} are {edge:?}.",
        edge = &front[..MESSAGE_SAMPLES.min(front.len())],
    );
}

/// **The reconstruction must not draw the wall's occlusion around what stands
/// in front of it.**
///
/// `ssao_upsample.slang` reads a channel gathered at [`OCCLUSION_EXTENT`] and
/// writes one at [`EXTENT`], so a full-resolution pixel beside a silhouette has
/// half-resolution neighbours on the *other* surface. Weighting those by
/// distance alone — a plain bilinear upsample — averages the background's
/// occlusion into the foreground's rim, which is a bright or dark fringe along
/// every silhouette in the frame.
///
/// # Why this check lives here and not beside the shader
///
/// `crcbl_shaders::ssao`'s `the_reconstruction_weighs_a_tap_by_its_depth` guards
/// the same property by looking for two expressions in the shader's source, and
/// its own doc comment says why it has to: with the depth weight replaced by a
/// constant, `crcbl`'s `render_e2e` still reported 31 of 32 passing with the same
/// one golden over tolerance, and lantern's golden still matched. The goldens
/// render at fixture sizes where the half-resolution grid is a few samples
/// across a silhouette, so the rim sits under every tolerance they carry — and
/// the frame it is visible in is the 1920×1080 one no golden runs at. **This
/// suite is that frame**, so the behaviour can be measured rather than grepped
/// for.
///
/// # The scene, and why it needs a third depth
///
/// A halo needs a background that is *occluded* and a foreground the filters
/// must not mix it into, and one lifted surface cannot be both: an occluder
/// inside [`RADIUS`] is what darkens the wall, and a surface inside the filters'
/// depth tolerance is one they are right to blend across. So the plate at
/// [`PLATE_LIFT`] casts the occlusion — the same plate
/// [`the_blurred_occlusion_falloff_does_not_terrace`] reads past — and a second
/// bar at [`HALO_LIFT`] stands in the darkest part of that falloff, far enough
/// forward that both filters' ramps are at zero across its edge.
///
/// The bar is a flat surface with nothing within [`RADIUS`] of it, so every
/// pixel of it is unoccluded and the honest reconstruction is one constant. What
/// is measured is therefore the **dip**: how far the darkest pixel inside the
/// bar falls below its brightest, which is the wall arriving through the
/// reconstruction and nothing else.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_reconstruction_does_not_halo_a_silhouette() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
    let (width, height) = EXTENT;
    let size = (width as f32, height as f32);
    let projection = Projection::Perspective {
        fov_y: FOV_Y,
        near: NEAR,
    }
    .matrix(size.0 / size.1);

    let wall = depth_of(projection, -WALL_Z);
    let plate = lifted_depth(projection, PLATE_LIFT);
    let bar = lifted_depth(projection, HALO_LIFT);
    let texels = depth_image(|x, _| {
        if PLATE_COLUMNS.contains(&x) {
            plate
        } else if HALO_COLUMNS.contains(&x) {
            bar
        } else {
            wall
        }
    });
    let run = run_passes(
        &headless,
        projection,
        &texels,
        crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
        1,
        crcbl::shaders::ssao::INTENSITY_DEFAULT,
        Chain::Reconstructed,
    );

    // The wall immediately left of the bar, and the bar itself — both at
    // [`EXTENT`], which is what the reconstruction wrote.
    let behind: Vec<u8> = (HALO_COLUMNS.start - HALO_MARGIN..HALO_COLUMNS.start)
        .map(|column| run.at(column, HALO_LINE_Y))
        .collect();
    let front: Vec<u8> = HALO_COLUMNS
        .map(|column| run.at(column, HALO_LINE_Y))
        .collect();
    assert_no_halo(&run, "columns", HALO_COLUMNS, &front, &behind);

    headless.finish();
}

/// **The same halo, down a column: a silhouette whose edge runs the other way
/// must not be blended across either.**
///
/// [`the_reconstruction_does_not_halo_a_silhouette`]'s scene turned a quarter
/// turn. The occluder is [`PLATE_ROWS`] and the foreground is [`HALO_ROWS`],
/// both spanning every column where that test's two span every row, and the
/// reconstruction is read down [`HALO_LINE_X`] instead of along
/// [`HALO_LINE_Y`]. The three depths, the chain, and both assertions are that
/// test's — see [`assert_no_halo`], which is the measurement both of them are.
///
/// # Why the other axis is a measurement and not a symmetry
///
/// `ssao_upsample.slang` runs its two axes independently: `span` is
/// `min(offset, int2(1, 1))` and the shares are `blend.x` and `blend.y`, one
/// factor per axis. So a transposed index, a `.x` where a `.y` was meant, or an
/// extent taken for the wrong axis is a defect that lands on one axis and
/// leaves the other exactly right. The sibling reads a row across a silhouette
/// that runs down the frame, so every tap it takes is displaced in `x`: it is
/// green on a reconstruction whose `y` axis blends straight across a
/// silhouette. This is the run that is not.
///
/// # One axis at a time
///
/// Both read lines sit on the gather's own grid — see [`HALO_LINE_X`] and
/// [`HALO_LINE_Y`] — so each test holds the axis it is not measuring at a
/// single tap and the two together are one measurement run once per axis.
///
/// # What it does not see
///
/// **One edge, one row of it.** [`HALO_ROWS`]'s far edge lands on the gather's
/// grid and reads its own sample alone, so what is measured is the leading
/// edge, exactly as the sibling measures the leading column of its bar. And the
/// halo is one row deep at the divisor that ships: the taps reach `nearest` and
/// `nearest + 1` and no further, so only the bar's first row can see a sample
/// from the wall — a larger `RESOLUTION_DIVISOR` would widen it, and the
/// 2026-09-02 sweep in [`MAX_HALO`] is what says that is where the anti-baseline
/// puts its dip on this axis too.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_reconstruction_does_not_halo_a_silhouette_down_a_column() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
    let (width, height) = EXTENT;
    let size = (width as f32, height as f32);
    let projection = Projection::Perspective {
        fov_y: FOV_Y,
        near: NEAR,
    }
    .matrix(size.0 / size.1);

    // The placement the measurement rests on, asserted rather than assumed —
    // the sliver test's `assert_ne!` on the same arithmetic, for the same
    // reason. A bar whose first row sat on the gather's grid would read one tap
    // and read it from its own surface, so nothing would be rejected and this
    // test would be green on a reconstruction with no depth weight at all; a
    // read column off that grid would put an `x` tap in the answer and stop the
    // pair being one axis each.
    let divisor = crcbl::shaders::ssao::RESOLUTION_DIVISOR;
    assert_ne!(
        HALO_ROWS.start % divisor,
        0,
        "the bar's first row {row} sits on the gather's grid, so `ssao_upsample.slang` reads one \
         tap and reads it from the bar's own sample. No tap is rejected there and the halo this \
         measures cannot arise however the taps are weighted. Move {row} off a multiple of \
         {divisor}.",
        row = HALO_ROWS.start,
    );
    assert_eq!(
        HALO_LINE_X % divisor,
        0,
        "the column {HALO_LINE_X} the bar is read down is off the gather's grid, so every tap \
         below is displaced in `x` as well as in `y` and a green run says nothing about the `y` \
         axis on its own. Move it onto a multiple of {divisor}."
    );

    let wall = depth_of(projection, -WALL_Z);
    let plate = lifted_depth(projection, PLATE_LIFT);
    let bar = lifted_depth(projection, HALO_LIFT);
    let texels = depth_image(|_, y| {
        if PLATE_ROWS.contains(&y) {
            plate
        } else if HALO_ROWS.contains(&y) {
            bar
        } else {
            wall
        }
    });
    let run = run_passes(
        &headless,
        projection,
        &texels,
        crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
        1,
        crcbl::shaders::ssao::INTENSITY_DEFAULT,
        Chain::Reconstructed,
    );

    // The wall immediately above the bar, and the bar itself — both at
    // [`EXTENT`], which is what the reconstruction wrote.
    let behind: Vec<u8> = (HALO_ROWS.start - HALO_MARGIN..HALO_ROWS.start)
        .map(|row| run.at(HALO_LINE_X, row))
        .collect();
    let front: Vec<u8> = HALO_ROWS.map(|row| run.at(HALO_LINE_X, row)).collect();
    assert_no_halo(&run, "rows", HALO_ROWS, &front, &behind);

    headless.finish();
}

/// **The AO intensity must move the occlusion, along the curve it names, and
/// must not move a frame that never asked for it.**
///
/// `ssao_upsample.slang`'s `ao_intensity` is the last thing the occlusion chain
/// does: the reconstructed visibility raised to `camera.params.z`, before
/// `mesh.slang` tints it. `docs/backlog.md` is where the knob is argued for —
/// the multi-bounce tint narrowed the contrast of every occluded surface, and a
/// power is what lets a frame ask for more occlusion than the horizon integral
/// measured rather than only less.
///
/// # What is asserted, and why a direction is not enough
///
/// A test that only watched the occlusion get darker would pass on a shader
/// that multiplied by a constant, or that applied the exponent to something
/// else. So each column of the falloff is **predicted** from its own reading at
/// the default and compared against what the device wrote: the exponent is a
/// per-pixel function of the visibility, so a window spanning most of the
/// falloff's range is a window where a wrong curve cannot fit. See
/// [`MAX_CURVE_ERROR`] for the tolerance and where it comes from.
///
/// [`MIN_INTENSITY_SWING`] is the other half. The prediction at the default is
/// the reading itself, so a knob that never reached the shader satisfies the
/// curve exactly at every intensity — the check has to see the window actually
/// move before the agreement means anything.
///
/// # The unwritten block
///
/// A producer that writes nothing leaves `params.z` as the padding it used to
/// be, which reads as zero, and every visibility raised to zero is one — a
/// frame with the occlusion silently switched off. The shader answers a zero
/// with `INTENSITY_DEFAULT` instead, and this asserts the whole channel comes
/// back **byte for byte** what the default produced, which is the strongest
/// form that claim has: not "close", the same image.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_ao_intensity_scales_the_reconstructed_occlusion() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
    let (width, height) = EXTENT;
    let size = (width as f32, height as f32);
    let projection = Projection::Perspective {
        fov_y: FOV_Y,
        near: NEAR,
    }
    .matrix(size.0 / size.1);

    // The falloff scene, which is [`the_blurred_occlusion_falloff_does_not_terrace`]'s:
    // a plate standing in front of a wall, read past its right edge. Nothing
    // here needs the halo test's second surface — the question is what happens
    // to a visibility, and this scene is where the reconstruction holds a range
    // of them.
    let wall = depth_of(projection, -WALL_Z);
    let plate = lifted_depth(projection, PLATE_LIFT);
    let texels = depth_image(|x, _| {
        if PLATE_COLUMNS.contains(&x) {
            plate
        } else {
            wall
        }
    });
    let at_intensity = |intensity: f32| {
        run_passes(
            &headless,
            projection,
            &texels,
            crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
            1,
            intensity,
            Chain::Reconstructed,
        )
    };
    let line = |run: &Run| -> Vec<u8> {
        INTENSITY_COLUMNS
            .map(|column| run.at(column, HALO_LINE_Y))
            .collect()
    };
    let mean = |line: &[u8]| -> f64 {
        line.iter().map(|level| f64::from(*level)).sum::<f64>() / line.len() as f64
    };

    let shipping = at_intensity(crcbl::shaders::ssao::INTENSITY_DEFAULT);
    let unwritten = at_intensity(0.0);
    let measured = line(&shipping);
    let base = mean(&measured);

    // Anti-vacuity before anything is predicted from it: a window of unoccluded
    // wall would satisfy every curve at once, since one raised to anything is
    // one.
    let deepest = measured
        .iter()
        .enumerate()
        .min_by_key(|(_, level)| **level)
        .map(|(step, _)| step)
        .expect("a non-empty window");
    let darkest = measured[deepest];
    let lightest = *measured.iter().max().expect("a non-empty window");
    assert!(
        lightest - darkest >= MIN_SWING,
        "the window at the default spans {darkest}..={lightest}, under the {MIN_SWING} levels \
         this measurement needs: an exponent applied to a constant visibility is a constant, so \
         there would be no curve here to check. Move {INTENSITY_COLUMNS:?} back into the plate's \
         falloff."
    );

    let mut worst = 0.0f64;
    let mut worst_at = (0.0f32, 0u32, 0u8, 0.0f64);
    let mut swings = Vec::new();
    for intensity in [
        crcbl::shaders::ssao::INTENSITY_MIN,
        crcbl::shaders::ssao::INTENSITY_MAX,
    ] {
        let run = at_intensity(intensity);
        let asked = line(&run);
        swings.push((
            intensity,
            f64::from(asked[deepest]) - f64::from(darkest),
            mean(&asked) - base,
        ));
        for (step, (measured_level, asked_level)) in measured.iter().zip(&asked).enumerate() {
            let visibility = f64::from(*measured_level) / f64::from(UNOCCLUDED);
            let predicted = visibility.powf(f64::from(intensity)) * f64::from(UNOCCLUDED);
            let error = (predicted - f64::from(*asked_level)).abs();
            if error > worst {
                worst = error;
                worst_at = (
                    intensity,
                    INTENSITY_COLUMNS.start + u32::try_from(step).expect("a window of columns"),
                    *asked_level,
                    predicted,
                );
            }
        }
    }

    eprintln!(
        "{suite}: the occlusion window {INTENSITY_COLUMNS:?} means {base:.1} at the default and \
         spans {darkest}..={lightest}; its darkest column moves {deep_min:+.0} at intensity {min} \
         and {deep_max:+.0} at {max} (the window's mean {mean_min:+.1} and {mean_max:+.1}), and \
         the worst column is {worst:.1} levels off the curve",
        suite = crate::SUITE,
        min = crcbl::shaders::ssao::INTENSITY_MIN,
        max = crcbl::shaders::ssao::INTENSITY_MAX,
        deep_min = swings[0].1,
        deep_max = swings[1].1,
        mean_min = swings[0].2,
        mean_max = swings[1].2,
    );

    assert_eq!(
        unwritten.image, shipping.image,
        "a block whose `params.z` was never written did not produce the frame the default \
         produces. Zero is what the padding this word used to be reads as, and every visibility \
         raised to zero is one — so the frame this run wrote has no occlusion in it at all. \
         `ao_intensity` in `ssao_upsample.slang` is what must answer a zero with \
         `INTENSITY_DEFAULT` rather than clamp it up to `INTENSITY_MIN`."
    );
    for (intensity, swing, _) in &swings {
        assert!(
            swing.abs() >= MIN_INTENSITY_SWING,
            "intensity {intensity} moved the window's darkest column, at {darkest}, by only \
             {swing:.0} levels — under the {MIN_INTENSITY_SWING} this test needs to be measuring \
             anything. The knob did not reach `camera.params.z`, or the reconstruction is not \
             reading it: every other assertion here passes on a frame the intensity never \
             touched, because the curve at the default predicts itself."
        );
    }
    assert!(
        swings[0].1 > 0.0 && swings[1].1 < 0.0,
        "the curve runs the wrong way: intensity {min} moved the darkest column {a:+.0} levels \
         and {max} moved it {b:+.0}. The channel carries visibility, so an exponent over one must \
         darken it and one under it must lighten — a knob that can only lighten is the blend \
         towards unoccluded `docs/backlog.md` refuses.",
        min = swings[0].0,
        max = swings[1].0,
        a = swings[0].1,
        b = swings[1].1,
    );
    assert!(
        worst <= MAX_CURVE_ERROR,
        "the reconstruction is not on the curve it was asked for: at intensity {intensity}, \
         column {column} reads {level} where its own default reading predicts {predicted:.1} — \
         {worst:.1} levels, against a bound of {MAX_CURVE_ERROR}. `ssao_upsample.slang` raises \
         the visibility to `camera.params.z`, so every column's reading at the default predicts \
         its reading at every other intensity.",
        intensity = worst_at.0,
        column = worst_at.1,
        level = worst_at.2,
        predicted = worst_at.3,
    );

    headless.finish();
}

/// **A surface thinner than the occlusion grid still gets an answer, and the
/// answer is the nearest sample there is.**
///
/// `ssao_upsample.slang`'s `NEAREST_TAP_FLOOR` is the share its nearest tap
/// keeps however far that tap's surface turns out to be, and the pixel it exists
/// for is the one this test builds: a foreground surface one column wide,
/// standing off the gather's grid, whose every neighbouring sample is on another
/// surface. Both of the shader's rejections then fire at once — the depth ramp
/// reaches zero for a tap the tolerance puts on another surface, and the far
/// plane takes no share at all — so without the floor the divisor is zero and
/// the pixel divides nothing by nothing.
///
/// # Why the assertion is an equality and not a tolerance
///
/// With every tap's `share` at zero, `max(share, NEAREST_TAP_FLOOR)` leaves the
/// nearest one holding the whole sum: `total` is that sample's own level times
/// the floor and `weight` is the floor, so the quotient is the sample itself.
/// The floor is a negative power of two, which makes both the multiply and the
/// divide exact; `INTENSITY_DEFAULT` takes the branch that returns the
/// visibility rather than raising it; and an `R8Unorm` level survives the round
/// trip through a float. So the reconstruction must write back the **same byte**
/// the gathered channel holds at that sample — which is what
/// [`Chain::Gathered`] is read for here, at [`OCCLUSION_EXTENT`], from a second
/// run of the same scene through the same passes.
///
/// # What this can and cannot see
///
/// [`MIN_SLIVER_TAP_CONTRAST`] is what makes the equality worth asserting: the
/// sliver's other tap is the sky, which gathers unoccluded, and its nearest is
/// occluded wall — so a reconstruction that stopped rejecting the far tap would
/// land tens of levels away and this goes red.
///
/// **The nearest tap's own rejection is not observable from the output**, and
/// this test does not pretend otherwise. That tap is the sole survivor either
/// way, so `total / weight` is its level whether its share is the floor or the
/// geometric weight it would have had — the two differ in `weight` alone and
/// `weight` cancels. What that share being zero decides is whether the
/// *divisor* is zero, and the only way to see that is to take the floor out of
/// the shader: done on 2026-09-02, this test fails and its siblings do not.
/// Everything asserted below about that tap is therefore asserted against the
/// depth image the scene was built from rather than against the frame.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_reconstruction_answers_a_sliver_with_its_nearest_sample() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
    let (width, height) = EXTENT;
    let size = (width as f32, height as f32);
    let projection = Projection::Perspective {
        fov_y: FOV_Y,
        near: NEAR,
    }
    .matrix(size.0 / size.1);

    // The falloff scene with the frame's right-hand side taken away: the plate
    // darkens the wall, the sliver stands one column wide in that darkness at
    // [`HALO_LIFT`]'s distance in front of it, and everything past the sliver is
    // the far plane — the sliver is the last surface in the frame, which is the
    // shortest way to spell "the tap on its far side has no surface at all".
    // What the reconstruction actually needs is only that one texel, and that is
    // what is asserted below; `ssao_blur.slang` leaves early at a far centre, so
    // the sample built on it reaches the reconstruction as unoccluded exactly.
    let wall = depth_of(projection, -WALL_Z);
    let plate = lifted_depth(projection, PLATE_LIFT);
    let sliver = lifted_depth(projection, HALO_LIFT);
    let texels = depth_image(|x, _| match x {
        x if PLATE_COLUMNS.contains(&x) => plate,
        x if x == SLIVER_COLUMN => sliver,
        x if x > SLIVER_COLUMN => crcbl::shaders::ssao::DEPTH_FAR,
        _ => wall,
    });

    // Where `ssao_upsample.slang` puts the sliver's taps: the sample the pixel
    // sits after, how far past it the pixel is, and the full-resolution texel
    // each of the two samples was gathered from. The shader's own arithmetic —
    // `pixel / RESOLUTION_DIVISOR`, and `full_res_pixel` back the other way.
    let divisor = crcbl::shaders::ssao::RESOLUTION_DIVISOR;
    let nearest = SLIVER_COLUMN / divisor;
    let offset = SLIVER_COLUMN % divisor;
    let nearest_texel = nearest * divisor;
    let along_texel = nearest_texel + divisor;

    // The placement, asserted rather than assumed. A sliver on one of the
    // gather's own texels is sampled by the gather, takes one tap, and takes it
    // from its own surface — which is a pixel the floor has nothing to do with,
    // and a scene this whole test would still be green on.
    assert_ne!(
        offset, 0,
        "the sliver at column {SLIVER_COLUMN} sits on the gather's own grid, so \
         `ssao_upsample.slang` reads one tap and reads it from the sliver's own sample. Nothing \
         is rejected there and the nearest-tap floor is never reached. Move {SLIVER_COLUMN} off a \
         multiple of {divisor}."
    );

    // The two surfaces the taps land on, read out of the image the scene was
    // built from. This is what says the taps are rejected: the nearer one is the
    // wall, which [`HALO_LIFT`] puts past both filters' depth tolerance from the
    // sliver, and the further one is the far plane, which takes no share at all.
    let depth_texel = |x: u32, y: u32| {
        let at = ((y * EXTENT.0 + x) * 4) as usize;
        f32::from_le_bytes(
            texels[at..at + 4]
                .try_into()
                .expect("four bytes of depth per texel"),
        )
    };
    for row in SLIVER_ROWS {
        assert_eq!(
            depth_texel(nearest_texel, row),
            wall,
            "the sliver's nearest tap at texel ({nearest_texel}, {row}) was gathered from \
             something other than the wall, so what rejects it is not the depth the scene was \
             built to reject it by"
        );
        assert_eq!(
            depth_texel(along_texel, row),
            crcbl::shaders::ssao::DEPTH_FAR,
            "the sliver's other tap at texel ({along_texel}, {row}) was gathered from a surface \
             rather than the far plane, so `ssao_upsample.slang`'s `depth <= DEPTH_FAR` is not \
             what rejects it and this scene is not the all-rejected one"
        );
        assert_eq!(
            depth_texel(SLIVER_COLUMN, row),
            sliver,
            "the sliver is not at column {SLIVER_COLUMN} on row {row}, so the pixel the \
             reconstruction is read at is not on the thin surface this test is about"
        );
    }

    // The same scene through the same passes, twice: once to the reconstruction
    // at [`EXTENT`] and once stopping at the blur, whose output is the image the
    // reconstruction taps.
    let reconstructed = run_passes(
        &headless,
        projection,
        &texels,
        crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
        1,
        crcbl::shaders::ssao::INTENSITY_DEFAULT,
        Chain::Reconstructed,
    );
    let gathered = run_passes(
        &headless,
        projection,
        &texels,
        crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
        1,
        crcbl::shaders::ssao::INTENSITY_DEFAULT,
        Chain::Gathered,
    );

    let mut contrast = u8::MAX;
    let mut written = Vec::new();
    let mut mismatched = Vec::new();
    for row in SLIVER_ROWS {
        let block = row / divisor;
        let near = gathered.at(nearest, block);
        let along = gathered.at(nearest + 1, block);
        contrast = contrast.min(near.abs_diff(along));
        let level = reconstructed.at(SLIVER_COLUMN, row);
        written.push(level);
        if level != near {
            mismatched.push((row, level, near, along));
        }
    }
    let rows = written.len();
    let darkest = *written.iter().min().expect("a non-empty band of rows");
    let lightest = *written.iter().max().expect("a non-empty band of rows");

    match reconstructed.pass("ssao-upsample") {
        Some(ns) => eprintln!(
            "{suite}: the ssao-upsample pass took {ns} ns at {width}×{height}; the sliver's \
             {rows} rows reconstruct to {darkest}..={lightest}, off gathered taps at least \
             {contrast} levels apart",
            suite = crate::SUITE,
        ),
        None => eprintln!(
            "{suite}: this device has no timestamp query, so the ssao-upsample pass is untimed \
             here; the sliver's {rows} rows reconstruct to {darkest}..={lightest}, off gathered \
             taps at least {contrast} levels apart",
            suite = crate::SUITE,
        ),
    }

    assert!(
        contrast >= MIN_SLIVER_TAP_CONTRAST,
        "the sliver's two taps are only {contrast} levels apart at their closest, under the \
         {MIN_SLIVER_TAP_CONTRAST} this measurement needs: the equality below asks for the \
         nearest tap's level, and a run whose other tap carries the same level satisfies it \
         however that tap was weighted. The wall at texel {nearest_texel} is not occluded enough \
         against the sky at {along_texel} — move {SLIVER_COLUMN} back into the plate's falloff."
    );
    assert!(
        mismatched.is_empty(),
        "the reconstruction did not answer the sliver with its nearest gathered sample: \
         {shown:?}, as `(row, written, nearest sample, other tap)`, of {count} rows in \
         {SLIVER_ROWS:?}. Every tap of this pixel is rejected — the nearest is wall past the \
         filters' depth tolerance and the other is the far plane — so \
         `ssao_upsample.slang`'s `NEAREST_TAP_FLOOR` is the whole divisor and \
         `total / weight` is that one sample exactly. A reading pulled towards the other tap is \
         a rejection that stopped rejecting; a reading that is neither is the divisor no longer \
         being the floor.",
        count = mismatched.len(),
        shown = &mismatched[..MESSAGE_SAMPLES.min(mismatched.len())],
    );

    headless.finish();
}
