// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/57-grass.md`'s rungs G1, G2 and G3, second half: the grass pass,
// drawing what `grass_gen.slang` generated as cards, as mesh blades, as shells
// and as fins.
//
// # Three looks' worth of entry points, one light walk
//
// Decision 3's looks differ in geometry and in nothing else, so they are
// entry points of one module rather than modules of their own: `vertexMain` and
// `fragmentMain` draw the cards, `bladeNearVertexMain`, `bladeFarVertexMain` and
// `bladeFragmentMain` the mesh blades at their two levels of detail,
// `shellVertexMain` and `finVertexMain` build the shells and their fins, and
// `shellFragmentMain` cuts both out. Every one of
// them is lit by `grass_light` and styled by `grass_style`, which is how
// decision 4's "any of the three looks can be stylised" is one function rather
// than a promise kept three times.
//
// # One draw per slot, and no vertex stream
//
// Decision 1: the geometry is built from `SV_VertexID` — two crossed quads of
// six vertices, standing on the blade's root and facing the direction its hash
// gave it. There is no vertex buffer and no index buffer, which is what lets a
// draw be nothing but a slot's instance count read out of a buffer a compute
// pass wrote.
//
// A slot's run is reached by the base the vertex stage adds to `SV_InstanceID`,
// not by the draw's `firstInstance`: WebGPU requires that word to be zero
// unless `indirect-first-instance` is enabled, and it is not.
//
// # Copied, and from where
//
// There is no `#include` in this tree, so the lighting this pass shares with
// the forward pass is a copy — the froxel column's arrangement (`docs/notes/
// rendering.md`), which `volumetric.slang` and `water.slang` are under. Copied
// here from `mesh.slang`, body for body:
//
// * `FrameUniforms` and `GpuLight`, bound as the very buffers the forward pass
//   is handed, so every field is read at the offset it was written at;
// * the sun's cascade walk, its receiver biases and its filters;
// * the punctual lights' walk — the falloff, the cone, the atlas tile and the
//   cube face.
//
// And from `grass_gen.slang` and `wind.slang`, for the shells: the placement
// hash, the ground under a point, the wind sampler and the lean it bends by —
// a shell layer bends by the very formula the card's tip does.
//
// `crcbl_shaders::grass`'s tests are where each is compared.
//
// # Shells, and why a fragment finds its strand by position
//
// Acerola's shell texturing, as the plan records it: a stack of layers over the
// ground, each a cutout of a per-cell strand that tapers with height. **The
// strand is the placement's blade.** Every cell `grass_gen.slang` grew a blade
// in is written to `grassCells` at the cell's own index, so a shell fragment
// knows its cell from where it stands and reads the blade's root, height,
// half-width, tint and row — the same numbers the card of that blade is drawn
// from. The strand is a cone about the root: its half-width at the root, bounded
// by half a cell, closing to nothing at the blade's height.
//
// Half a cell is the bound because a fragment then needs only the four cells
// nearest it: a strand of that reach rooted anywhere in a cell cannot touch a
// point further away. A fin, standing at the centre of a band of cells, reads
// the band across and the two nearest cells along it.
//
// **The shell index is the instance index**, decision 3's "one instanced draw
// rather than one per layer", counted from the top so that a layer nearer the
// camera from above is drawn before the ones it hides.
//
// # Mesh blades
//
// Decision 3's realistic look: a cubic Bézier per blade, built from
// `SV_VertexID` as a triangle strip of a pair of vertices at each of several
// heights and one at the tip — fifteen near and seven far. Tilt and facing set
// the tip, the bow sets the two middle points, the wind's lean is added as the
// Bernstein form of the card's own `t²` curve, and the normal is the curve's
// derivative crossed with the blade's side, tilted out toward each edge so a
// flat blade shades round.
//
// **Every blade stays at least a pixel wide.** A flat blade seen edge-on
// projects to nothing, and a strip narrower than a pixel is a coverage decision
// taken off a rasteriser's last bits — the failure G1's per-instance card level
// and G3's strand floor were each written for. So the width is laid along a
// direction that turns from the blade's own side toward the one perpendicular to
// the view as the blade turns edge-on, and is then floored at
// `GRASS_BLADE_MIN_PIXELS` of the depth it is drawn at.
//
// **The two levels meet without a pop.** In the band before the switch the near
// level first narrows away three blades in four — the ones the far level does
// not keep — while widening the fourth to the far level's width, and then moves
// each vertex onto the far level's straight segments. At the switch the near
// strip lies on the far strip, and a blade crossing it changes which draw it is
// in and not where its pixels are.
//
// # Fins
//
// A shell stack seen at a grazing angle shows the gaps between its layers.
// NVIDIA's shells and fins extrude a fin along every silhouette edge in a
// geometry stage, which WebGPU has none of; here every fin is a pre-built strip
// of quads from `SV_VertexID`, and `finVertexMain` collapses the ones whose two
// sides' ground normals both face the camera squarely. A fin that stays draws
// the side view of the strands rooted in its band.
//
// # What this pass does *not* copy, and why
//
// **No BRDF.** A blade is Lambert diffuse over the ground's normal, plus the
// frame's flat ambient. Decision 4's levers are colour and normal, and decision
// 3 has even the near blades "lose gloss" with distance; a specular lobe on a
// card is a highlight on a billboard, which is the thing card grass is worst
// at. `docs/backlog.md` carries it.
//
// **No area light, no probe, no contact shadow, no screen-space occlusion and
// no fog.** Each is a binding and a copy of its own, and each is a term rather
// than the walk this rung's row names. A rectangle in a blade's froxel is
// skipped outright rather than shaded as something it is not.
//
// # No motion vector
//
// Decision 1's last line — Ghost of Tsushima wrote none for grass, and this
// pass attaches only the scene colour and the scene depth. The motion and
// reflectivity targets keep what the forward pass cleared them to, which is
// exactly "this pixel does not move" and "this pixel reflects nothing".

// D3D12 takes each resource's register from its binding. A declaration's
// `D3D12_REGISTER` suffix is `register(<class><binding>, space<set>)`, the
// numbers its `[[vk::binding(binding, set)]]` names, and
// `crcbl_shaders::declaration_order` checks that the two agree. HLSL only,
// because Slang's Metal target would take its argument-table indices from a
// `register` as well.
#if defined(CRCBL_TARGET_HLSL)
#define D3D12_REGISTER(...) : register(__VA_ARGS__)
#else
#define D3D12_REGISTER(...)
#endif

static const uint SHADOW_CASCADES = 2;

static const uint SHADOW_LIGHT_TILES = 14;

static const uint SHADOW_POINT_FACES = 6;

static const uint SHADOW_ATLAS_TILES = 16;

static const uint KIND_DIRECTIONAL = 0;

static const uint KIND_POINT = 1;

static const uint KIND_SPOT = 2;

static const uint KIND_RECT = 3;

static const uint CLUSTER_LIGHT_CAPACITY = 16;

static const uint CLUSTER_STRIDE = 17;

static const uint CLUSTER_DEPTH_SLICES = 24;

static const float CLUSTER_NEAR = 0.1;

static const float CLUSTER_FAR = 1000.0;

static const uint PROBE_LEVELS = 4;

/// The most shells a field stacks: the length of `GrassField::layers`.
/// `crcbl_shaders::grass::MAX_SHELLS`.
static const uint GRASS_MAX_SHELLS = 64;

/// `mesh.slang`'s frame block, bound here as the very buffer the forward pass
/// read: the camera, the ambient, the sun's cascades and the light grid.
struct FrameUniforms
{
    /// World → clip. Reversed-Z: 1.0 at the near plane, 0.0 at infinity.
    float4x4 view_proj;
    /// World-space eye position in `xyz`; `w` unused.
    float4 camera_position;
    /// Flat ambient term in `rgb`. The irradiance grid below is **added** to it
    /// rather than replacing it.
    ///
    /// **Ambient is not a light and is not a row**: it stands in for every
    /// bounce the direct terms do not carry, so it has no position, no cluster
    /// and no shadow. The sun used to sit beside it here as a direction and a
    /// colour, and it is a row in `lights` now — see that binding.
    ///
    /// **`w` is the normals view's switch** — `NORMALS_VIEW` below — and this is
    /// the lane it rides in because it is the only spare one every writer of this
    /// block already leaves at exactly zero. `camera_position.w` is written `1.0`
    /// by both of them, so a switch there would have turned the debug view on for
    /// every frame the engine has ever drawn; a `float4` of its own at the end of
    /// the block would move no offset either, and was not taken because the block
    /// is constructed field-by-field by a test outside this crate's workspace
    /// neighbourhood — `crates/crcbl/tests/forward_e2e/depth_probe.rs` — which a
    /// new member breaks. A lane every writer already zeroes is the change that
    /// costs nothing anywhere.
    float4 ambient;
    /// World → cascade `i`'s shadow clip, one matrix per cascade.
    ///
    /// Orthographic and reversed-Z like every other projection here, so a
    /// caster at the light's near plane stores 1.0 and the far plane stores 0.0
    /// — which is why the comparison below is `Greater` and not `Less`.
    ///
    /// **The shadow pass reads this block through `view_proj`, not through
    /// here.** Cascade `i` is rendered by binding a second copy of this whole
    /// struct whose `view_proj` *is* `shadow_view_proj[i]`, which is what lets
    /// the depth-only pipeline reuse `vertexMain` and `mesh_cluster.slang`'s
    /// mesh stage unchanged rather than being a second transform path that could
    /// disagree with the first. This array is the *fragment* stage's copy, for
    /// putting a shaded point back into the map's space.
    float4x4 shadow_view_proj[SHADOW_CASCADES];
    /// Component `i` is how far from the eye cascade `i` reaches, in world
    /// units; a fragment picks the first cascade whose distance covers it.
    ///
    /// Only the first `SHADOW_CASCADES` components are read. The rest are
    /// written as the last real split so an out-of-range read — which cannot
    /// happen, but would be silent if it did — selects the widest cascade rather
    /// than a zero that shadows everything.
    float4 cascade_far;
    /// `x`: one shadow-atlas texel in `u`. `y`: one texel in `v`. `z`: how many
    /// **cascade texels** the sun's shadow comparison is moved towards the
    /// light. `w`: how many of them it is moved along the receiver's own
    /// geometric normal, per unit of `sin(acos(Ng·L))` — the normal-offset
    /// bias, and `shadow_normal_offset` is what argues the direction.
    ///
    /// Both texel sizes are carried even where the grid is square and they are
    /// equal: the kernel below steps in tile space and scales back by the grid's
    /// two extents, and the grid stops being square the moment a point light's
    /// six tiles arrive. Deriving one from the other is how the PCF kernel ends
    /// up sampling a rectangle.
    ///
    /// **`z` and `w` are counts of texels, not depths.** `sun_visibility` is
    /// what converts them, through the world footprint of one texel of whichever
    /// cascade the fragment landed in — the same denomination
    /// `PUNCTUAL_DEPTH_BIAS_TEXELS` carries for a cone and a cube face, so the
    /// sun and the punctual lights are biased in one unit rather than two. They
    /// are the sun's numbers all the same: a cascade's footprint is a different
    /// arithmetic from a frustum's, and only the cascades read these.
    float4 shadow_params;
    /// The froxel grid's extent: `x` froxels across the screen, `y` down it, `z`
    /// depth slices, `w` how many pixels wide and tall one tile is.
    ///
    /// The slice count is `CLUSTER_DEPTH_SLICES` for a perspective camera and
    /// `1` for an orthographic one, which has no view depth to slice by. The
    /// tile size is `crcbl_shaders::light::CLUSTER_TILE_PIXELS` at any viewport
    /// the grid buffer can hold and is doubled where it cannot, which is what
    /// keeps a very large frame coarser rather than out of bounds. The
    /// distribution of the slices over depth is a constant this file and
    /// `light_cluster.slang` each declare, so there is nothing there for the two
    /// to disagree about.
    ///
    /// This slot held topic 25's selection numbers until the light
    /// list arrived, by which time no stage read them: the projection moved into
    /// `draw_gen.slang`, which owns the hysteresis state it needs, and
    /// `mesh_cluster.slang`'s amplification stage reads that pass's answer rather
    /// than the numbers. A dead `float4` in a block every pipeline binds is not
    /// somewhere to leave a value nothing reads.
    uint4 cluster_grid;
    /// World → light tile `i`'s shadow clip, one matrix per tile of the atlas's
    /// light region.
    ///
    /// **Perspective and reversed-Z**, unlike `shadow_view_proj` above: a spot
    /// is a cone, a cone is a frustum, a point light's face is a 90° one, and the
    /// orthographic box the cascades use is neither. Reversed all the same, so
    /// the same `Greater` comparison sampler reads all three.
    ///
    /// **Indexed by tile rather than by light**, which is what lets one light own
    /// six: `GpuLight::shadow_tile` names the first of a light's tiles and
    /// `point_visibility` adds the face it selected. A tile no light holds is
    /// read by nothing, because the rows that would name it carry
    /// `NO_SHADOW_TILE`. **Last in the block rather than beside the cascades**,
    /// so adding it moved no existing member's offset — which is what let the
    /// cascade goldens stay byte-identical across the change that introduced it.
    float4x4 light_view_proj[SHADOW_LIGHT_TILES];
    /// `xyz`: how many probes **each level** of the irradiance clipmap holds on
    /// each axis — the same count every level, which is what makes a coarser
    /// level cover more world rather than hold more probes. `w`: how many rows
    /// `probes` holds across every level, and the bound every fetch below is
    /// clamped against. Matching `crcbl_shaders::probe::ProbeVolume`.
    ///
    /// **The rows below were appended after these**, each on
    /// `light_view_proj`'s terms: no existing member's offset moved, so every
    /// golden blessed before it existed still matched. The clipmap is the one
    /// change that did not keep that discipline — growing this group from three
    /// rows to `2 · PROBE_LEVELS + 2` moved every member after it — and it could
    /// not: what moved is the *shape* of a member the block already had, and
    /// nothing after it moved relative to anything else. No golden moved either,
    /// because the offsets are a contract between this file and
    /// `crcbl_shaders::mesh::FRAME_UNIFORMS_SIZE` and the two changed together.
    ///
    /// All zero for a scene with no probes, and `probe_irradiance` then returns
    /// exactly zero with no branch anywhere — see it.
    uint4 probe_counts;
    /// `x`: how many clipmap levels are live, never more than `PROBE_LEVELS`.
    /// `y`: how many rows one level holds, which is also where each next level's
    /// rows begin. `z` and `w` are the sixteen-byte row's padding.
    uint4 probe_levels;
    /// Where probe `(0, 0, 0)` of each level stands, in world space; `w` unused.
    ///
    /// **Uploaded rather than derived here.** Where a level stands is one rule,
    /// `crcbl_shaders::probe::ProbeVolume::level_origin` is where it is written,
    /// and this stage reads the answer — exactly as `probe_octahedral.slang`
    /// reads a direction table rather than evaluating the octahedral mapping a
    /// second time. The rows past `probe_levels.x` are zeroed and nothing reads
    /// them.
    float4 probe_level_origin[PROBE_LEVELS];
    /// One over each level's spacing on each axis; `w` unused.
    ///
    /// Inverted on the host so this stage multiplies rather than divides. An
    /// axis whose reciprocal is zero collapses onto probe 0, which is what the
    /// grid of a scene with no probes carries.
    float4 probe_level_inv_spacing[PROBE_LEVELS];
    /// How far each level has scrolled, as whole probe steps already reduced
    /// into `0..probe_counts`; `w` unused.
    ///
    /// The irradiance probes' scrolling: a level re-centres on
    /// the camera by whole probe steps, `probe_level_origin` above has already
    /// moved with it, and this is the wrap that leaves the probes which stayed
    /// naming the rows they already had. `probe_wrap` is the one place it is
    /// applied.
    ///
    /// **Reduced on the host**, by
    /// `crcbl_shaders::probe::ProbeVolume::level_offset`, so a cell and an
    /// offset both sit below the count and the wrap here is one compare and one
    /// subtract rather than a signed remainder. All zero for a volume that has
    /// never scrolled, which is every volume before this row existed.
    uint4 probe_level_offset[PROBE_LEVELS];
    /// Topic 25's selection numbers, as `draw_gen.slang`'s
    /// `DrawGenParams::lod_params` carries them: `x` pixels per unit of length
    /// one unit from the eye, `y` the budget a group's projected error must
    /// exceed to expand, `z` the budget an expanded one is held down to, `w`
    /// unread padding.
    ///
    /// **Read by nothing in this file.** It is declared because this block and
    /// `mesh_cluster.slang`'s are one buffer, and *that* file's mesh stage reads
    /// all three for the screen-error heatmap — it projects a group's error
    /// itself and anchors the ramp on the two budgets. A block short of this row
    /// here would be one whose size the two stages disagree about.
    ///
    /// **Last in the block**, on `probe_origin`'s terms exactly: no existing
    /// member's offset moves, so every golden blessed before this member existed
    /// still matches.
    float4 lod_params;
    /// Exponential height fog: `x` the density at the reference height, per
    /// world unit travelled; `y` the height over which that density falls by a
    /// factor of `e`; `z` the reference height itself; `w` unread.
    ///
    /// **A zero density is exactly no fog.** `fog_optical_depth` is then
    /// exactly zero, `fog_transmittance` of zero is exactly one, and the
    /// composite below is `lit * t + fog * (1 - t)` — which at `t == 1` returns
    /// `lit` bit for bit, where a `lerp` would return `lit + (fog - lit)` and
    /// lose the low bits of an HDR value far from the fog colour. So this is a
    /// feature made of data with no branch selecting it, on the probe grid's
    /// terms, and no golden blessed before it existed moves.
    ///
    /// **Last in the block**, on `lod_params`' terms: no existing member's
    /// offset changes.
    float4 fog_params;
    /// The radiance the fog scatters towards the eye, in `rgb`; `w` unread.
    ///
    /// Pre-tonemap and unclamped like everything else this stage writes: it is
    /// a radiance rather than a display colour, so a bright sky's fog may sit
    /// above one and the bloom chain may see it.
    float4 fog_color;
    /// The red channel of the sky's L1 irradiance, packed as `GpuProbe::sh_r`
    /// is: the linear band in `xyz` and the constant band in `w`, so
    /// `sky_irradiance` evaluates it as `dot(row, float4(N, 1))`.
    ///
    /// A distant environment reaches a diffuse surface in exactly this shape,
    /// which is why the sky and the irradiance grid share it. The two are
    /// **added** rather than chosen between: a scene may have a sky above it
    /// and bounce light around it, and both are the same term.
    ///
    /// **Zero is exactly off.** Three zero rows make three zero dot products,
    /// so the sum the ambient term starts from is the one it was before a sky
    /// existed. `crcbl_shaders::sky::SkyGradient::irradiance` is the host side
    /// that fills these, and `SkyGradient::BLACK` projects to exactly zero.
    ///
    /// **Last in the block**, on `lod_params`' terms.
    float4 sky_sh_r;
    /// The green channel of the same. See `sky_sh_r`.
    float4 sky_sh_g;
    /// The blue channel of the same. See `sky_sh_r`.
    float4 sky_sh_b;
    /// World → clip **as the previous frame's camera saw it**: the camera-side
    /// twin of `GpuInstance::previous_transform`, and the other half of what a
    /// motion vector is made of.
    ///
    /// `crcbl_render::ForwardRenderer` advances it once per frame, at the same
    /// boundary `crcbl_render::InstancePool::rotate` settles a transform's
    /// previous value on, and the first frame carries its own `view_proj` — a
    /// camera that has not moved yet has not moved.
    ///
    /// **Last in the block**, on `lod_params`' terms: no existing member's
    /// offset moves, so every golden blessed before this member existed still
    /// matches.
    float4x4 previous_view_proj;
    /// Where the pool's **attribute** region begins, as a word index; `yzw`
    /// unread padding. See `crcbl_shaders::mesh::FrameUniforms::vertex_pool`,
    /// and `load_vertex` above, which is the only reader.
    uint4 vertex_pool;
    /// Where atlas slot `i`'s map is in the shadow atlas: a scale into the image
    /// in `xy` and an offset in `zw`, so a point `t` of that map's own `0..1`
    /// space is at `zw + t * xy`. `atlas_rect` is what reads it.
    ///
    /// **What replaced a tile index into a fixed grid**,
    /// topic 45's atlas rung: this file used to derive a cell
    /// from the index, so every map was one size and could only be that size.
    /// Reading the rectangle the host allocated lets a far or small light take a
    /// halving of a cell with no second sampling path here.
    ///
    /// Indexed by **slot** rather than by light, on `light_view_proj`'s terms:
    /// the cascades are the first `SHADOW_CASCADES` rows and `light_tile` of a
    /// light's tile is its own. A slot no map was rendered into carries a zero
    /// rectangle and is read by nothing, because the rows that could name it
    /// carry `NO_SHADOW_TILE`.
    ///
    /// **Last in the block**, on `lod_params`' terms: no existing member's
    /// offset moves.
    float4 shadow_atlas_rect[SHADOW_ATLAS_TILES];
    /// Which shadow filter each side of the comparison seam runs: `x` the near
    /// side's mode, `y` the far side's, `z` the seam's column in pixels and `w`
    /// unread padding.
    ///
    /// The modes are `SHADOW_FILTER_PCSS`, `SHADOW_FILTER_DISC` and
    /// `SHADOW_FILTER_BOX` below, and `shadow_filter_mode` is the only reader:
    /// a fragment left of `z` takes `x` and one at or past it takes `y`.
    /// `crcbl_render::split` owns where that column falls and
    /// `crcbl_render::shadow`'s `r_shadow_filter` owns which mode is on the near
    /// side.
    ///
    /// **A frame comparing nothing carries one mode in both lanes and a zero
    /// column**, so every fragment reads `y` and there is no seam anywhere. All
    /// zeroes is that frame at `SHADOW_FILTER_PCSS`, which is what ships and
    /// what every golden in this workspace was blessed under — so a writer that
    /// leaves this row alone draws the picture it drew before the row existed.
    ///
    /// **Last in the block**, on `lod_params`' terms: no existing member's
    /// offset moves.
    uint4 shadow_filter;
};

/// `mesh.slang`'s light row, bound here as the very buffer the forward pass
/// read.
struct GpuLight
{
    /// World-space position in `xyz`, influence radius in `w`.
    float4 position;
    /// Colour premultiplied by intensity in `rgb`; `w` unused.
    float4 color;
    /// Towards the light for a directional; the cone axis for a spot; the
    /// direction a rectangle emits along. `w` is the cosine of a spot's outer
    /// half-angle, and a rectangle's half-extent along its `v` axis.
    float4 direction;
    /// A rectangle's unit `u` axis in `xyz` and its half-extent along it in `w`.
    /// Unread by every other kind. The `v` axis is
    /// `cross(tangent.xyz, direction.xyz)`, which is unit because those two are.
    float4 tangent;
    uint kind;
    /// The cosine of a spot's inner half-angle.
    float cos_inner;
    /// The **first** entry of `FrameUniforms::light_view_proj` this light
    /// occludes through, or `NO_SHADOW_TILE` if it was given none.
    ///
    /// A spot occludes through that one tile; a point light through the
    /// `SHADOW_POINT_FACES` tiles from here, and `point_visibility` adds the face
    /// it selected. The atlas holds `SHADOW_LIGHT_TILES` of them and a scene may
    /// want more than they hold, so a row saying "no tile" is the ordinary case:
    /// `docs/plan/18-render-features.md`'s rule is that such a light still lights
    /// and simply does not occlude.
    uint shadow_tile;
    /// What this light may contribute: `FLAG_FILL`, or zero for an ordinary one.
    uint flags;
};

/// What this pass needs that is not a buffer's contents, matching
/// `crcbl_shaders::grass::Params`.
struct GrassParams
{
    /// `x` how many instances one tile slot holds, which is the stride a
    /// slot's run begins at. `y` how many rows `blades` holds. `zw` are written
    /// as zero and unread.
    uint4 limits;
    /// `x` how many pixels a metre spans at one unit of view depth — half the
    /// viewport's width times the projection's own `x` scale — which is what
    /// the vertex stage sizes a card by. `yzw` are written as zero and unread.
    float4 screen;
};

/// Which tile this draw is reading, at the same dynamic offset its generation
/// dispatch was given. `crcbl_shaders::grass`'s
/// `the_two_shaders_declare_one_tile_block` is where the two are compared.
struct GrassTile
{
    /// `xy` the world XZ of the tile's minimum corner, `z` the tile's side in
    /// metres, `w` written as zero and unread.
    float4 tile;
    /// `x` the tile's slot, `y` its index along `+X`, `z` its index along `+Z`,
    /// `w` written as zero and unread.
    uint4 slot;
};

/// The blade description a cover map's `g` channel indexes — one row per look a
/// field mixes. `crcbl_shaders::grass::GrassBlade`.
///
/// Decision 3's "one blade description": cards, mesh blades and shells differ in
/// the geometry a row is drawn with and not in the row — `flags.x` names which.
struct GrassBlade
{
    /// The colour at the blade's root in `rgb`; `a` is written as zero and
    /// unread.
    float4 root_color;
    /// The colour at its tip in `rgb`; `a` is written as zero and unread.
    float4 tip_color;
    /// `x` the blade's height in metres, `y` its half-width in metres, `z` the
    /// fraction of the height a blade may lose to its own hash and `w` the
    /// fraction of the half-width it may lose.
    float4 size;
    /// Decision 4's root occlusion: the colour a blade darkens toward at its
    /// root in `rgb`, and in `a` the fraction of its height that reaches. A
    /// reach of zero is exactly no occlusion.
    float4 occlusion;
    /// The additive tip colour in `rgb`, and in `a` the fraction of the height
    /// it starts at. A colour of zero adds exactly nothing.
    float4 glow;
    /// The colour a patch leans toward in `rgb`, and in `a` the most of it a
    /// patch takes. A share of zero is exactly no patch.
    float4 patch;
    /// A mesh blade's shape: `x` its tilt, `y` its bow and `z` its rounding.
    /// `w` is written as zero and unread.
    float4 shape;
    /// Decision 2's clumps, for every look: `x` the share of a blade's facing
    /// its clump's replaces, `y` the fraction of its height a clump may take
    /// away. `zw` are written as zero and unread.
    float4 clump;
    /// `x` the look — `GRASS_LOOK_CARDS`, `GRASS_LOOK_SHELLS` or
    /// `GRASS_LOOK_BLADES` — and `y` the normal it shades by. `zw` are written
    /// as zero and unread.
    uint4 flags;
};

/// One generated blade — what a generation dispatch appends and what a draw
/// reads. `crcbl_shaders::grass::GrassInstance`.
struct GrassInstance
{
    /// The world-space root in `xyz` and the blade's height in metres in `w`.
    float4 root;
    /// The unit facing on the XZ plane in `xy`, the half-width in metres in `z`
    /// and the blade's own tint lane — a hashed `0..1` — in `w`.
    float4 facing;
    /// The metres the tip is moved by the wind, in world space. Exactly zero
    /// where the intensity layer is calm, which is what makes "calm means
    /// still" a property of a multiply rather than of a branch.
    float4 lean;
    /// The ground's unit normal under the root in `xyz`; `w` is written as zero
    /// and unread. Decision 4's lever: a card shades by the ground's normal, so
    /// a field reads as one surface rather than as a crowd of billboards.
    float4 ground;
    /// Where the blade stands from its clump's point, in metres along `+X` in
    /// `x` and `+Z` in `y`. `zw` are written as zero and unread.
    float4 clump;
    /// `x` the blade's cell in the field, which is the lane every hash below is
    /// drawn from and the key a determinism comparison sorts on. `y` the row of
    /// `blades` it was given. `z` its clump's identity, `w` one where the far
    /// level of detail keeps it and zero where it does not.
    uint4 lanes;
};

[[vk::binding(0, 0)]]
ConstantBuffer<FrameUniforms> frame D3D12_REGISTER(b0, space0);

[[vk::binding(1, 0)]]
ConstantBuffer<GrassParams> grass D3D12_REGISTER(b1, space0);

[[vk::binding(2, 0)]]
ConstantBuffer<GrassTile> tile D3D12_REGISTER(b2, space0);

/// Every slot's instances, as `grass_gen.slang` appended them: the cards from
/// the start of the slot's first region, the far blades from its end, and the
/// near blades from the start of its second — see that file's header.
[[vk::binding(3, 0)]]
StructuredBuffer<GrassInstance> instances D3D12_REGISTER(t3, space0);

/// The blade rows, indexed by `GrassInstance::lanes.y`.
[[vk::binding(4, 0)]]
StructuredBuffer<GrassBlade> blades D3D12_REGISTER(t4, space0);

/// The card's coverage-preserving mip chain — decision 3's "Castaño's
/// coverage-preserving alpha mipmaps are cooked at build time so a distant card
/// keeps its density". `crcbl_render::grass::card` is what cooks it.
///
/// **Sampled at a level the vertex stage chose**, not at one the hardware's own
/// derivatives picked, and that is a correctness decision rather than a cost
/// one — see `grassCardLevel`. Golus's in-shader compensation, which the cooked
/// chain replaces, is a separate thing and is not here: the level below selects
/// *which* level to read, and the chain is what makes every level draw the same
/// density once read.
///
/// An array of one layer rather than a plain `Texture2D` because
/// `crcbl_render::texture` carries a mip chain only on its array form, and a
/// second upload path is a second place D3D12's 256-byte row pitch has to be
/// got right.
[[vk::binding(5, 0)]]
Texture2DArray<float4> grassCard D3D12_REGISTER(t5, space0);

[[vk::binding(6, 0)]]
SamplerState grassCardSampler D3D12_REGISTER(s6, space0);

/// The shadow atlas and its comparison sampler, as `mesh.slang` binds them.
[[vk::binding(7, 0)]]
DepthTexture2D shadow_atlas D3D12_REGISTER(t7, space0);

[[vk::binding(8, 0)]]
SamplerComparisonState shadow_sampler D3D12_REGISTER(s8, space0);

/// The light rows, as `mesh.slang` binds them.
[[vk::binding(9, 0)]]
StructuredBuffer<GpuLight> lights D3D12_REGISTER(t9, space0);

/// The froxel lists, as `mesh.slang` binds them: a count and then that many
/// indices into `lights`, `CLUSTER_STRIDE` words apart.
[[vk::binding(10, 0)]]
StructuredBuffer<uint> cluster_lights D3D12_REGISTER(t10, space0);

/// A field's static block: where its placement lattice is, the ground under it
/// and its shell stack. `crcbl_shaders::grass::FieldBlock`.
struct GrassField
{
    /// `xy` the world XZ of the field's minimum corner, `z` a tile's side and
    /// `w` a placement cell's side, both in metres.
    float4 origin;
    /// `x` tiles along `+X`, `y` along `+Z`, `z` cells along a tile's side and
    /// `w` instances one slot holds.
    uint4 tiles;
    /// `grass_gen.slang`'s `GrassGenParams::ground`, the same four numbers.
    float4 ground;
    /// `grass_gen.slang`'s `GrassGenParams::maps`, the same four numbers.
    uint4 maps;
    /// `x` the shell stack's height in metres, `y` how many shells stand in it,
    /// `z` one where fins stand and zero where they do not. `w` unread.
    float4 stack;
    /// `x` the distance from the camera, in metres, at which a mesh blade
    /// changes level of detail, and `y` the band before it the near level drops
    /// and morphs over. `zw` unread.
    float4 blades;
    /// One row per shell, lowest first: `x` its height as a fraction of the
    /// stack and `y` the occlusion it is lit by — decision 3's per-shell
    /// constants, evaluated on the CPU so no `pow` reaches this file.
    float4 layers[GRASS_MAX_SHELLS];
};

[[vk::binding(11, 0)]]
ConstantBuffer<GrassField> field D3D12_REGISTER(b11, space0);

/// Every cell's blade at the cell's own index, zero where it carries none —
/// `grass_gen.slang`'s `grassCells`, read here by the shells and fins.
[[vk::binding(12, 0)]]
StructuredBuffer<GrassInstance> grassCells D3D12_REGISTER(t12, space0);

/// The ground the field stands on, which a shell's sheet is lifted off. Read
/// with `Load`, on `grass_gen.slang`'s terms.
[[vk::binding(13, 0)]]
Texture2D<float> grassGround D3D12_REGISTER(t13, space0);

/// `wind.slang`'s uniform block, bound here as the very buffer the generation
/// pass reads.
struct WindParams
{
    /// Unit horizontal direction the wind blows towards, on the XZ plane.
    float2 baseDirection;
    /// Base speed in m/s — `S` in decision 6's formula.
    float baseSpeed;
    /// Peak fractional swing of the gust about one. Zero is no gusting.
    float gustAmplitude;
    /// `dot(camera - o, baseDirection) / wavelength`, whole turns dropped.
    float gustPhase;
    /// One over the gust wavelength, in reciprocal metres.
    float invGustWavelength;
    /// Written as zero and unread; the row `std140` leaves.
    float2 pad0;
    /// The camera's texture coordinate in the direction layer, wrapped.
    float2 directionUv;
    /// Direction-layer texture coordinates per metre of world.
    float2 directionUvPerMetre;
    /// The camera's texture coordinate in the intensity layer, wrapped.
    float2 intensityUv;
    /// Intensity-layer texture coordinates per metre of world.
    float2 intensityUvPerMetre;
};

[[vk::binding(14, 0)]]
ConstantBuffer<WindParams> wind D3D12_REGISTER(b14, space0);

/// The wind's two layers and its sampler, as `grass_gen.slang` binds them.
[[vk::binding(15, 0)]]
Texture2D<float4> windDirectionLayer D3D12_REGISTER(t15, space0);

[[vk::binding(16, 0)]]
Texture2D<float4> windIntensityLayer D3D12_REGISTER(t16, space0);

[[vk::binding(17, 0)]]
SamplerState windSampler D3D12_REGISTER(s17, space0);

static const float PUNCTUAL_DEPTH_BIAS_TEXELS = 2.0;

static const float PUNCTUAL_NORMAL_OFFSET_TEXELS = 4.0;

static const uint SHADOW_TAPS = 32;

static const float SHADOW_FILTER_TEXELS = 2.0;

static const float2 SHADOW_DISC[SHADOW_TAPS] = {
    float2(+0.125000, +0.000000), float2(-0.159645, +0.146248),
    float2(+0.024436, -0.278438), float2(+0.201222, +0.262459),
    float2(-0.369268, -0.065318), float2(+0.349802, -0.222516),
    float2(-0.117002, +0.435242), float2(-0.223136, -0.429634),
    float2(+0.484115, +0.176798), float2(-0.503641, +0.207896),
    float2(+0.242788, -0.518824), float2(+0.179414, +0.572001),
    float2(-0.540757, -0.313380), float2(+0.634370, -0.139464),
    float2(-0.387146, +0.550675), float2(-0.089440, -0.690200),
    float2(+0.549072, +0.462758), float2(-0.738878, +0.030555),
    float2(+0.538955, -0.536332), float2(-0.036058, +0.779792),
    float2(-0.512818, -0.614527), float2(+0.812360, +0.109302),
    float2(-0.688311, +0.478909), float2(+0.188086, -0.836061),
    float2(+0.435033, +0.759191), float2(-0.850448, -0.271316),
    float2(+0.826102, -0.381680), float2(-0.357888, +0.855156),
    float2(-0.319407, -0.888034), float2(+0.849909, +0.446688),
    float2(-0.944035, +0.248845), float2(+0.536596, -0.834530),
};

static const uint SHADOW_PROBE_TAPS = 5;

static const uint SHADOW_PROBE_INDEX[SHADOW_PROBE_TAPS] = { 0u, 23u, 25u, 27u, 29u };

static const float2 SHADOW_ROTATIONS[16] = {
    float2(+1.000000, +0.000000), float2(+0.923880, +0.382683),
    float2(+0.707107, +0.707107), float2(+0.382683, +0.923880),
    float2(+0.000000, +1.000000), float2(-0.382683, +0.923880),
    float2(-0.707107, +0.707107), float2(-0.923880, +0.382683),
    float2(-1.000000, +0.000000), float2(-0.923880, -0.382683),
    float2(-0.707107, -0.707107), float2(-0.382683, -0.923880),
    float2(-0.000000, -1.000000), float2(+0.382683, -0.923880),
    float2(+0.707107, -0.707107), float2(+0.923880, -0.382683),
};

static const uint SHADOW_DITHER[16] = {
    0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u,
};

static const float SHADOW_CASTER_REACH = 40.0;

static const float SHADOW_SUN_TAN_RADIUS = 0.02;

static const float SHADOW_SEARCH_TEXELS = 8.0;

static const uint SHADOW_SEARCH_TAPS = 16;

static const float2 SHADOW_SEARCH_DISC[SHADOW_SEARCH_TAPS] = {
    float2(+0.176777, +0.000000), float2(-0.225772, +0.206826),
    float2(+0.034558, -0.393771), float2(+0.284571, +0.371173),
    float2(-0.522223, -0.092374), float2(+0.494695, -0.314685),
    float2(-0.165466, +0.615525), float2(-0.315561, -0.607594),
    float2(+0.684642, +0.250030), float2(-0.712256, +0.294009),
    float2(+0.343354, -0.733729), float2(+0.253730, +0.808932),
    float2(-0.764746, -0.443186), float2(+0.897134, -0.197232),
    float2(-0.547507, +0.778772), float2(-0.126487, -0.976090),
};

static const uint SHADOW_FILTER_PCSS = 0u;

static const uint SHADOW_FILTER_DISC = 1u;

static const uint SHADOW_FILTER_BOX = 2u;

static const float CASCADE_FADE_FRACTION = 0.1;

static const uint CASCADE_NONE = SHADOW_CASCADES;

/// Vertices one card instance draws: two crossed quads of six.
/// `crcbl_shaders::grass::CARD_VERTICES`, and `grass_gen.slang` writes it into
/// every slot's draw arguments.
static const uint GRASS_CARD_VERTICES = 12;

/// Vertices in one of those quads.
static const uint GRASS_CARD_QUAD_VERTICES = 6;

/// The alpha a card's coverage must reach to draw.
///
/// **The cook's number as much as the shader's**: `crcbl_render::grass::card`
/// bisects each mip's scale so the fraction of its texels reaching exactly this
/// is the fraction the top level's did, so a cutoff here that drifted from the
/// one the chain was cooked at would throw away the property the chain exists
/// for. `crcbl_shaders::grass::ALPHA_CUTOFF` is the Rust mirror.
static const float GRASS_ALPHA_CUTOFF = 0.5;

/// How much of its row's colour a blade may lose to its own tint lane.
/// `crcbl_shaders::grass::TINT_RANGE`.
static const float GRASS_TINT_RANGE = 0.18;

/// Texels along one side of the card's top level.
/// `crcbl_shaders::grass::CARD_EXTENT`.
static const uint GRASS_CARD_EXTENT = 64;

/// Levels in the card's chain, down to a single texel.
/// `crcbl_shaders::grass::CARD_LEVELS`.
static const uint GRASS_CARD_LEVELS = 7;

/// Which level of the card's chain a blade `pixels` wide on screen reads.
///
/// **The vertex stage chooses it, and that is what keeps two rasterisers
/// drawing one picture.** Left to the hardware, the level comes from the uv
/// derivatives of a quad seen nearly edge-on, and Vulkan lets an implementation
/// approximate that — `max(|du/dx|, |dv/dx|)` where another takes the square
/// root of the sum of squares, which differ by half a level at the grazing
/// angles a field of cards is all made of. Half a level is nothing in a blend
/// and everything through a cutout: it is a *binary* per-pixel decision, and
/// two conformant drivers then draw the same field with different blades in it.
/// Measured over `Scene::Meadow` on 2026-09-16 — 6% of the frame's pixels past
/// the golden's per-channel tolerance, against 1.2% with the chain pinned to
/// its top level — which is what this function was written for. Chosen per
/// instance, the level is the same number for every fragment of a card, so what
/// a driver's last bit can move is *which card* changes level and not which
/// pixel.
///
/// **No transcendental**, which decision 9 asks of anything reaching a pixel:
/// the level is found by walking the octaves and interpolating linearly inside
/// the one that contains the ratio. That is the piecewise-linear approximation
/// of `log2` — exact at every octave boundary and at most 0.086 of a level away
/// between them, which is a blend weight nothing can see.
float grassCardLevel(float pixels)
{
    float ratio = float(GRASS_CARD_EXTENT) / max(pixels, 1e-4);
    float level = 0.0;
    for (uint step = 1; step < GRASS_CARD_LEVELS; ++step)
    {
        float lower = float(1u << (step - 1u));
        if (ratio >= lower)
        {
            level = float(step - 1u) + min((ratio - lower) / lower, 1.0);
        }
    }
    // **Rounded to a whole level, and the cook is what makes that affordable.**
    // A fractional level blends two of them, and a blend weight is a number two
    // implementations may round differently — through a cutout, differently
    // enough to change which texels draw. A whole level has no weight to
    // disagree about. What it costs elsewhere is a card popping from one level
    // to the next as it recedes, and that pop is a change of *shape* rather
    // than of density precisely because every level of this chain passes the
    // same fraction of its texels — which is the whole of what the cook buys.
    return min(floor(level + 0.5), float(GRASS_CARD_LEVELS - 1u));
}

/// The numbers the shells, fins and levers are built from, each the Rust
/// constant of the same name in `crcbl_shaders::grass` without its `GRASS_`
/// prefix — `the_shaders_spell_this_module_s_constants` compares them.
static const uint GRASS_SHELL_GRID = 16;

static const uint GRASS_FIN_SPACING = 4;

static const uint GRASS_FIN_SEGMENTS = 16;

static const uint GRASS_FIN_ROWS = 4;

static const uint GRASS_LOOK_SHELLS = 1;

static const uint GRASS_NORMAL_UP = 1;

static const uint GRASS_LOOK_BLADES = 2;

static const uint GRASS_BLADE_NEAR_SEGMENTS = 7;

static const uint GRASS_BLADE_FAR_SEGMENTS = 3;

static const uint GRASS_CLUMP_CELLS = 8;

static const float GRASS_BLADE_MIN_PIXELS = 1.0;

static const float GRASS_BLADE_FAR_WIDEN = 2.0;

static const float GRASS_CLUMP_DOME = 0.5;

static const uint GRASS_PATCH_SALT = 0x165667b1u;

static const float GRASS_FIN_GRAZE_START = 0.5;

static const float GRASS_FIN_GRAZE_FULL = 0.3;

static const float GRASS_STRAND_MIN_PIXELS = 1.0;

/// `grass_gen.slang`'s bend constants, copied so a shell layer bends by the
/// formula a card's tip does.
static const float GRASS_MAX_BEND = 0.6;

static const float GRASS_BEND_HALF_SPEED = 6.0;

static const float WIND_MIN_DIRECTION_LENGTH_SQUARED = 1e-12;

/// The two triangles of a quad, as corners in `0..1`: `x` across the card and
/// `y` up it, zero at the root.
static const float2 GRASS_CARD_CORNERS[GRASS_CARD_QUAD_VERTICES] = {
    float2(0.0, 0.0), float2(1.0, 0.0), float2(0.0, 1.0),
    float2(1.0, 0.0), float2(1.0, 1.0), float2(0.0, 1.0),
};

uint light_tile(uint tile)
{
    return SHADOW_CASCADES + tile;
}

float shadow_normal_offset(float3 geometric_normal, float3 to_light)
{
    float cosine = saturate(dot(geometric_normal, to_light));
    return sqrt(saturate(1.0 - cosine * cosine));
}

float2 shadow_rotation(float2 pixel)
{
    uint2 cell = uint2(pixel) & 3u;
    return SHADOW_ROTATIONS[SHADOW_DITHER[cell.y * 4u + cell.x]];
}

uint shadow_filter_mode(float2 pixel)
{
    return uint(pixel.x) < frame.shadow_filter.z ? frame.shadow_filter.x
                                                 : frame.shadow_filter.y;
}

float2 atlas_uv(float4 rect, float2 tile_uv)
{
    return rect.zw + tile_uv * rect.xy;
}

float4 atlas_rect(uint tile)
{
    return frame.shadow_atlas_rect[tile];
}

float2 atlas_step(float4 rect)
{
    return frame.shadow_params.xy / rect.xy;
}

float tile_texels(float4 rect)
{
    return rect.x / frame.shadow_params.x;
}

bool atlas_rect_is_empty(float4 rect)
{
    return !(rect.x > 0.0);
}

float tile_tap(float4 rect, float2 texel_step, float2 tile_uv, float2 spoke, float2 rotation,
               float reference)
{
    float2 tile_min = float2(0.5, 0.5) * texel_step;
    // The rotation, written out: a complex multiply by `(cos, sin)`.
    float2 turned = float2(spoke.x * rotation.x - spoke.y * rotation.y,
                           spoke.x * rotation.y + spoke.y * rotation.x);
    float2 tap = clamp(tile_uv + turned * texel_step, tile_min, 1.0 - tile_min);
    return shadow_atlas.SampleCmpLevelZero(shadow_sampler, atlas_uv(rect, tap), reference);
}

float tile_pcf(uint tile, float2 tile_uv, float reference, float2 pixel, float radius)
{
    float2 rotation = shadow_rotation(pixel);
    float4 rect = atlas_rect(tile);
    if (atlas_rect_is_empty(rect))
    {
        return 1.0;
    }
    float2 texel_step = atlas_step(rect);

    float probe = 0.0;
    for (uint spot = 0u; spot < SHADOW_PROBE_TAPS; ++spot)
    {
        probe += tile_tap(rect, texel_step, tile_uv,
                          SHADOW_DISC[SHADOW_PROBE_INDEX[spot]] * radius, rotation, reference);
    }
    // Unanimity, tested at both ends rather than by a tolerance: a comparison
    // sampler returns exactly zero or exactly one wherever the four texels it
    // filters agree, whatever weights it gave them, so the sum of five is an
    // exact integer on every fragment this early-out is meant for.
    if (probe <= 0.0)
    {
        return 0.0;
    }
    if (probe >= float(SHADOW_PROBE_TAPS))
    {
        return 1.0;
    }

    float visibility = 0.0;
    for (uint index = 0u; index < SHADOW_TAPS; ++index)
    {
        visibility += tile_tap(rect, texel_step, tile_uv, SHADOW_DISC[index] * radius, rotation,
                               reference);
    }
    return visibility / float(SHADOW_TAPS);
}

float tile_box_pcf(uint tile, float2 tile_uv, float reference)
{
    float4 rect = atlas_rect(tile);
    if (atlas_rect_is_empty(rect))
    {
        return 1.0;
    }
    float2 texel_step = atlas_step(rect);

    float visibility = 0.0;
    for (int y = -1; y <= 1; ++y)
    {
        for (int x = -1; x <= 1; ++x)
        {
            visibility += tile_tap(rect, texel_step, tile_uv, float2(float(x), float(y)),
                                   float2(1.0, 0.0), reference);
        }
    }
    return visibility / 9.0;
}

float sun_penumbra_texels(uint cascade, float2 tile_uv, float reference, float2 rotation)
{
    float2 texel = frame.shadow_params.xy;
    float4 rect = atlas_rect(cascade);
    float2 texel_step = atlas_step(rect);
    float2 tile_min = float2(0.5, 0.5) * texel_step;
    // The atlas's extent in texels, which is what turns a normalised atlas
    // coordinate into the integer `Load` wants.
    float2 extent = float2(1.0, 1.0) / texel;

    float sum = 0.0;
    float found = 0.0;
    for (uint index = 0u; index < SHADOW_SEARCH_TAPS; ++index)
    {
        float2 spoke = SHADOW_SEARCH_DISC[index] * SHADOW_SEARCH_TEXELS;
        float2 turned = float2(spoke.x * rotation.x - spoke.y * rotation.y,
                               spoke.x * rotation.y + spoke.y * rotation.x);
        float2 tap = clamp(tile_uv + turned * texel_step, tile_min, 1.0 - tile_min);
        // `min` against the last texel rather than trust to the clamp above: that
        // one is in tile space and half a texel inside, so the product below
        // lands inside the atlas by construction — but it is a float product,
        // and an out-of-range `Load` is undefined rather than clamped.
        int2 coord = int2(min(atlas_uv(rect, tap) * extent, extent - float2(1.0, 1.0)));
        float depth = shadow_atlas.Load(int3(coord, 0));
        if (depth > reference)
        {
            sum += depth;
            found += 1.0;
        }
    }

    if (found <= 0.0)
    {
        return SHADOW_FILTER_TEXELS;
    }

    // Shadow-clip depth back to metres. The cascade's box is `2 * radius` deep
    // through the sphere plus `SHADOW_CASTER_REACH` in front of it, and an
    // orthographic projection spreads that range linearly over `0..1`, so one
    // unit of `ndc.z` is the whole of it. `cascade_matrix` on the host is where
    // those two ends are set.
    float radius = frame.cascade_far[cascade];
    float separation = (sum / found - reference) * (2.0 * radius + SHADOW_CASTER_REACH);
    float texel_world = 2.0 * radius / tile_texels(rect);
    return clamp(separation * SHADOW_SUN_TAN_RADIUS / texel_world,
                 SHADOW_FILTER_TEXELS,
                 SHADOW_SEARCH_TEXELS);
}

float cascade_visibility(uint cascade, float3 world_position, float3 to_light,
                         float3 geometric_normal, float2 pixel)
{
    // The bias, and it is `punctual_visibility`'s exactly: two moves of the
    // world position before it is projected, each a multiple of the world
    // footprint of one texel of *this cascade's* map. A cascade is an
    // orthographic box of `2 * radius` fitted to `tile_texels` of its own
    // rectangle, and its radius is the split distance the caller's selection
    // read — the same arithmetic `crcbl_render::shadow::cascade_matrix` snaps
    // its origin to. The side comes out of the rectangle rather than out of a
    // constant because a map is no longer always a whole root cell; see
    // `tile_texels`.
    //
    // **A number in texels rather than in shadow-clip depth**, which is the
    // whole of this: acne is the map quantising a receiver's own depth away
    // across one texel, so the error to cover scales with the texel's footprint
    // and with the surface's slope. A constant added in clip instead scales with
    // the cascade's whole depth range — `2 * radius` plus `CASTER_REACH`, tens
    // of metres of it belonging to casters standing off the near plane — so the
    // same number meant 0.83 m of world slack on the outer cascade and lit a
    // strip along the foot of every wall.
    //
    // **The two moves are in different directions and cover different things.**
    // Along the normal is `shadow_normal_offset`'s, and it is what the slope
    // costs: it walks the lookup sideways across the map instead of raising the
    // depth it compares, so a grazing surface is read where it belongs and a
    // contact shadow keeps its contact. Along the light is the constant, and it
    // is the facet *seam* — two triangles of different slope biased by different
    // amounts, where the texel their shared edge falls in stores the steeper
    // one's depth and no offset read off either facet predicts the other's.
    //
    // What the two counts are and why they are what they are lives on the host,
    // in `crcbl_render::shadow`: `DEPTH_BIAS_TEXELS` and `NORMAL_OFFSET_TEXELS`
    // have the numbers and the frames they were read off.
    float4 rect = atlas_rect(cascade);
    if (atlas_rect_is_empty(rect))
    {
        // No map in this slot, so nothing here was ever rendered. **Before the
        // divide**, which is the whole point: see `atlas_rect_is_empty`.
        return 1.0;
    }
    float texel_world = 2.0 * frame.cascade_far[cascade] / tile_texels(rect);
    float3 biased = world_position
                  + geometric_normal
                        * (texel_world * frame.shadow_params.w
                           * shadow_normal_offset(geometric_normal, to_light))
                  + to_light * (texel_world * frame.shadow_params.z);

    float4 clip = mul(frame.shadow_view_proj[cascade], float4(biased, 1.0));
    // Orthographic, so `w` is 1 and the divide is a formality — written out
    // anyway because nothing in this file may assume which constructor built a
    // matrix, which is `Projection`'s whole point on the CPU side.
    float3 ndc = clip.xyz / clip.w;
    if (any(abs(ndc.xy) > 1.0) || ndc.z <= 0.0)
    {
        // Outside this cascade's box, or beyond its far plane. Lit: a shadow map
        // knows nothing about what it never rendered, and inventing a shadow
        // there is a black band across everything past the last split.
        return 1.0;
    }

    // Y-up NDC to top-left texture space, which is the flip `crcbl_render`'s
    // projections push into the viewport rather than into the matrix.
    float2 tile_uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);

    // Both moves are already in `ndc`, and they land in different components.
    // The one along `to_light` moved the reference *up* — reversed-Z, so a
    // larger depth is nearer the light, which is the direction that stops a
    // surface shadowing itself — and it cannot move `tile_uv`, because the
    // projection's own depth axis is that direction. The one along the normal is
    // the opposite: it is perpendicular to that axis in the amount
    // `shadow_normal_offset` returns, so what it moves is `tile_uv` and the
    // texel this fragment reads. That is the difference between raising a
    // comparison and taking a different one.
    // The filter's width, which for the sun is not a constant: a contact is
    // sharp and a shadow thrown from height is soft, and `sun_penumbra_texels`
    // is what reads the difference out of the map. The rotation is taken once
    // and handed to both so the disc the search walks is the disc the filter
    // draws.
    uint mode = shadow_filter_mode(pixel);
    if (mode == SHADOW_FILTER_BOX)
    {
        return tile_box_pcf(cascade, tile_uv, ndc.z);
    }
    if (mode == SHADOW_FILTER_DISC)
    {
        // The disc at its fixed reach: the ninth decision without the tenth, so
        // the penumbra is the same width wherever its caster stands.
        return tile_pcf(cascade, tile_uv, ndc.z, pixel, SHADOW_FILTER_TEXELS);
    }
    float2 rotation = shadow_rotation(pixel);
    float radius = sun_penumbra_texels(cascade, tile_uv, ndc.z, rotation);
    return tile_pcf(cascade, tile_uv, ndc.z, pixel, radius);
}

float sun_visibility(float3 world_position, float3 to_light, float n_dot_l,
                     float3 geometric_normal, float2 pixel, out uint selected,
                     out float fade)
{
    // **The two out-parameters are what the cascade view draws, and they are
    // written on every path out of here.** The alternative was for
    // `fragmentMain` to run this selection a second time beside the shading,
    // which is exactly the arrangement an overlay must not have: an overlay
    // derived independently can disagree with the lighting it claims to explain,
    // and the disagreement is invisible — a plausible tint over a plausible
    // shadow. Reported rather than recomputed, the tint *is* the selection.
    selected = CASCADE_NONE;
    fade = 0.0;
    if (n_dot_l <= 0.0)
    {
        return 1.0;
    }

    // The first cascade whose reach covers this fragment. Distance to the eye
    // rather than view-space z: the splits are radii around the camera, so a
    // fragment at the edge of the screen and one straight ahead at the same
    // distance land in the same cascade and the seam is a sphere rather than a
    // plane the camera can rotate.
    float eye_distance = length(world_position - frame.camera_position.xyz);
    uint cascade = SHADOW_CASCADES - 1;
    // Whether a cascade's reach actually covers this fragment, as against the
    // fallback above. The two are the same *index* and a different fact: past
    // the last split the shading still asks the last cascade — and gets "lit"
    // out of its box test — while the overlay must say "no cascade covers this",
    // because a tint there would colour the whole distance as if it were mapped.
    bool covered = false;
    for (uint index = 0; index < SHADOW_CASCADES; ++index)
    {
        if (eye_distance < frame.cascade_far[index])
        {
            cascade = index;
            covered = true;
            break;
        }
    }
    if (covered)
    {
        selected = cascade;
    }

    float visibility =
        cascade_visibility(cascade, world_position, to_light, geometric_normal, pixel);
    if (cascade + 1 >= SHADOW_CASCADES)
    {
        // The last cascade has nothing to fade into. Everything past its reach
        // selects it too — the loop's fallback — and gets its answer unmixed,
        // which the box test inside turns into "lit" soon enough.
        return visibility;
    }

    // The band, at the outer edge of this cascade's reach. `reach` is strictly
    // positive here: the loop only leaves a cascade selected whose far distance
    // *exceeds* this fragment's, and the one case where it does not — nothing
    // matched — returned above.
    float reach = frame.cascade_far[cascade];
    float band = reach * CASCADE_FADE_FRACTION;
    float blend = saturate((eye_distance - (reach - band)) / band);
    // Before the early return, not after it: `saturate` has already made a
    // fragment short of the band an exact zero, and reporting it there as well
    // is what keeps this parameter written on every path.
    fade = blend;
    if (blend <= 0.0)
    {
        return visibility;
    }

    // The next cascade is guaranteed to contain this fragment, which is what
    // makes the mix well defined rather than a blend with the `1.0` an outside
    // fragment gets: the cascades are spheres about the same eye with strictly
    // growing radii — `crcbl_render::shadow::cascade_matrix` fits every one of
    // them to `eye` — so anything inside cascade `i` is inside cascade `i + 1`
    // by construction, not by a margin someone has to maintain.
    float next =
        cascade_visibility(cascade + 1, world_position, to_light, geometric_normal, pixel);
    return lerp(visibility, next, blend);
}

float punctual_visibility(uint tile, float3 world_position, float3 to_light, float n_dot_l,
                          float map_world, float3 geometric_normal, float2 pixel)
{
    uint atlas = light_tile(tile);
    float4 rect = atlas_rect(atlas);
    if (atlas_rect_is_empty(rect))
    {
        // `cascade_visibility`'s guard, for its reason: a row naming a slot the
        // host laid no map into divides by zero here otherwise.
        return 1.0;
    }
    float texel_world = map_world / tile_texels(rect);
    // Two moves, in two directions, on `sun_visibility`'s terms exactly: along
    // the normal by what the receiver's slope costs in lateral travel, and along
    // the light by the constant that covers a facet seam.
    float3 biased = world_position
                  + geometric_normal
                        * (texel_world * PUNCTUAL_NORMAL_OFFSET_TEXELS
                           * shadow_normal_offset(geometric_normal, to_light))
                  + to_light * (texel_world * PUNCTUAL_DEPTH_BIAS_TEXELS);

    float4 clip = mul(frame.light_view_proj[tile], float4(biased, 1.0));
    // **`w` first, and this is the difference from the cascades.** An
    // orthographic matrix has `w == 1` everywhere; a perspective one has `w`
    // equal to the depth in front of the light, so a receiver *behind* the light
    // gets a negative `w` and a divide that mirrors it into the frustum. Without
    // this, a surface behind a spot samples the map as though it were in front
    // of it.
    if (clip.w <= 0.0)
    {
        return 1.0;
    }
    float3 ndc = clip.xyz / clip.w;
    if (any(abs(ndc.xy) > 1.0) || ndc.z <= 0.0 || ndc.z > 1.0)
    {
        // Outside this map's own frustum, nearer than its near plane or past its
        // far one. Lit, on `sun_visibility`'s terms: a map knows nothing about
        // what it never rendered. A cone's falloff has already closed by the
        // frustum's edge and a face's neighbour covers what it does not, so this
        // path costs no picture.
        return 1.0;
    }

    // Y-up NDC to top-left texture space, the flip `crcbl_render`'s projections
    // push into the viewport rather than into the matrix.
    float2 tile_uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    // The constant width, not the sun's measured one: a punctual map is a
    // perspective projection, so a difference of two of its depths is not a
    // distance until it is un-projected, and its light has no angular radius to
    // scale one by. Contact hardening for these is its own rung.
    //
    // **`pcss` and `disc` are the same filter here**, which is why only the box
    // is a branch: the tenth decision is the sun's alone, so the two modes above
    // it differ on a cascade and not on a cone or a cube face.
    if (shadow_filter_mode(pixel) == SHADOW_FILTER_BOX)
    {
        return tile_box_pcf(atlas, tile_uv, ndc.z);
    }
    return tile_pcf(atlas, tile_uv, ndc.z, pixel, SHADOW_FILTER_TEXELS);
}

float spot_visibility(GpuLight light, uint tile, float3 world_position, float3 to_light,
                      float n_dot_l, float3 geometric_normal, float2 pixel)
{
    if (n_dot_l <= 0.0)
    {
        // Facing away: black from the diffuse term already, and the place
        // self-shadowing acne is worst. `sun_visibility`'s reason exactly.
        return 1.0;
    }

    // The cone's outer half-angle, as a tangent, out of the cosine the row
    // carries: `tan(acos(c)) == sqrt(1 - c²) / c`. `Light::row` refuses a
    // non-positive one on the way in and `Selection` refuses a cone at or past
    // 80°, so the divide has a real denominator.
    float cos_outer = light.direction.w;
    float tan_outer = sqrt(saturate(1.0 - cos_outer * cos_outer)) / max(cos_outer, 1e-4);
    // Depth along the cone's axis, which is what the projection's `w` will be —
    // computed here with one dot product because the offset has to be applied
    // *before* the matrix, and a radial distance would be up to `1 / cos_outer`
    // too large at the rim.
    float axial = max(dot(world_position - light.position.xyz, normalize(light.direction.xyz)), 0.0);
    return punctual_visibility(tile, world_position, to_light, n_dot_l, 2.0 * tan_outer * axial,
                               geometric_normal, pixel);
}

uint point_face(float3 from_light)
{
    float3 axis = abs(from_light);
    if (axis.x >= axis.y && axis.x >= axis.z)
    {
        return from_light.x >= 0.0 ? 0u : 1u;
    }
    if (axis.y >= axis.z)
    {
        return from_light.y >= 0.0 ? 2u : 3u;
    }
    return from_light.z >= 0.0 ? 4u : 5u;
}

float point_visibility(GpuLight light, uint base, float3 world_position, float3 to_light,
                       float n_dot_l, float3 geometric_normal, float2 pixel)
{
    if (n_dot_l <= 0.0)
    {
        return 1.0;
    }

    float3 from_light = world_position - light.position.xyz;
    // A face is a 90° frustum, so its half-angle's tangent is 1 and the axial
    // depth is the largest component of the offset — which is the same
    // `2 d tan θ` a spot computes, with both terms known rather than read off
    // the row.
    float axial = max(max(abs(from_light.x), abs(from_light.y)), abs(from_light.z));
    return punctual_visibility(base + point_face(from_light), world_position, to_light, n_dot_l,
                               2.0 * axial, geometric_normal, pixel);
}

float range_window(float distance, float radius)
{
    float ratio = distance / max(radius, 1e-6);
    float window = saturate(1.0 - ratio * ratio * ratio * ratio);
    return window * window;
}

float punctual_falloff(float distance, float radius)
{
    return range_window(distance, radius) / (distance * distance + 1.0);
}

float spot_cone(float3 to_light, float3 axis, float cos_outer, float cos_inner)
{
    float cosine = dot(-to_light, normalize(axis));
    // `max` rather than a guard: a caller writing the two cosines the wrong way
    // round would otherwise divide by a negative and light the outside of the
    // cone. `crcbl_render::SpotLight` refuses that pairing on the way in; this
    // is the shader-side half of the same statement.
    return saturate((cosine - cos_outer) / max(cos_inner - cos_outer, 1e-4));
}

uint froxel_of(float2 pixel, float depth)
{
    uint grid_x = max(frame.cluster_grid.x, 1u);
    uint grid_y = max(frame.cluster_grid.y, 1u);
    uint slices = max(frame.cluster_grid.z, 1u);
    uint tile_pixels = max(frame.cluster_grid.w, 1u);

    // Clamped as well as divided: the grid is `ceil(extent / tile)` wide, so the
    // last column and row are part-empty and a pixel in the part that is not
    // there would index the next row's froxel.
    uint tile_x = min(uint(pixel.x) / tile_pixels, grid_x - 1);
    uint tile_y = min(uint(pixel.y) / tile_pixels, grid_y - 1);

    float scale = float(CLUSTER_DEPTH_SLICES) / log2(CLUSTER_FAR / CLUSTER_NEAR);
    float bias = -scale * log2(CLUSTER_NEAR);
    // Clamped *before* the logarithm as well as after: a fragment on the eye
    // plane has zero depth, and `log2(0)` is an infinity whose conversion to an
    // integer is undefined rather than merely wrong.
    float slice = floor(log2(max(depth, CLUSTER_NEAR)) * scale + bias);
    uint index = uint(clamp(slice, 0.0, float(slices - 1)));

    return (index * grid_y + tile_y) * grid_x + tile_x;
}


/// `wind.slang`'s smoothed triangle wave, copied through `grass_gen.slang`.
float windSmoothTriangle(float u)
{
    float s = abs(frac(u + 0.5) * 2.0 - 1.0);
    return s * s * (3.0 - 2.0 * s);
}

/// `wind.slang`'s sampler, copied body for body.
float3 windSample(float3 posRel)
{
    float2 deflection =
        windDirectionLayer
            .SampleLevel(windSampler, wind.directionUv + posRel.xz * wind.directionUvPerMetre, 0.0)
            .rg
            * 2.0
        - 1.0;
    float intensity =
        windIntensityLayer
            .SampleLevel(windSampler, wind.intensityUv + posRel.xz * wind.intensityUvPerMetre, 0.0)
            .r;

    float2 base = wind.baseDirection;
    float2 turned = float2(base.x * deflection.x - base.y * deflection.y,
                           base.x * deflection.y + base.y * deflection.x);
    float lengthSquared = dot(turned, turned);
    float2 direction = lengthSquared > WIND_MIN_DIRECTION_LENGTH_SQUARED
                           ? turned / sqrt(lengthSquared)
                           : base;

    float along = dot(posRel.xz, base) * wind.invGustWavelength + wind.gustPhase;
    float gust = 1.0 + wind.gustAmplitude * (2.0 * windSmoothTriangle(along) - 1.0);
    float speed = intensity * wind.baseSpeed * gust;
    return float3(direction.x * speed, 0.0, direction.y * speed);
}

/// `grass_gen.slang`'s lean, copied: how far the wind moves the tip of a blade
/// `height` metres tall.
float3 grass_lean(float3 velocity, float height)
{
    float speed = length(velocity);
    float bend = height * GRASS_MAX_BEND * (speed / (speed + GRASS_BEND_HALF_SPEED));
    float2 along = velocity.xz * (bend / max(speed, 1e-6));
    float drop = bend * bend / (height + sqrt(max(height * height - bend * bend, 0.0)));
    return float3(along.x, -drop, along.y);
}

/// `grass_gen.slang`'s placement hash, copied — the patch lane below is drawn
/// from it.
uint grass_hash(uint value)
{
    uint state = value * 747796405u + 2891336453u;
    uint word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

/// `grass_gen.slang`'s pair of sixteen-bit lanes, copied.
float2 grass_unit_pair(uint lane)
{
    return float2(float(lane & 0xffffu), float((lane >> 16u) & 0xffffu)) * (1.0 / 65536.0);
}

/// The height and the ground's unit normal under a world XZ position.
struct GrassGround
{
    /// Metres above the origin.
    float height;
    /// The ground's unit normal.
    float3 normal;
};

/// `grass_gen.slang`'s texel of the ground field, copied.
float grass_ground_texel(int2 texel)
{
    int2 last = int2(int(field.maps.x) - 1, int(field.maps.y) - 1);
    return grassGround.Load(int3(clamp(texel, int2(0, 0), last), 0));
}

/// `grass_gen.slang`'s ground under a point, copied — so a shell's sheet stands
/// on the height a card's root was placed at.
GrassGround grass_ground_under(float2 world)
{
    float2 at = (world - field.ground.xy) * field.ground.w;
    float2 base = floor(at);
    float2 blend = at - base;
    int2 texel = int2(base);
    float h00 = grass_ground_texel(texel);
    float h10 = grass_ground_texel(texel + int2(1, 0));
    float h01 = grass_ground_texel(texel + int2(0, 1));
    float h11 = grass_ground_texel(texel + int2(1, 1));

    GrassGround under;
    float lower = h00 + blend.x * (h10 - h00);
    float upper = h01 + blend.x * (h11 - h01);
    under.height = lower + blend.y * (upper - lower);
    float rise_x = 0.5 * ((h10 - h00) + (h11 - h01));
    float rise_z = 0.5 * ((h01 - h00) + (h11 - h10));
    under.normal = normalize(float3(-rise_x, field.ground.z, -rise_z));
    return under;
}

/// Which colour patch a blade of clump `clump` is painted with, as a lane in
/// `0..1` — decision 4's clump-coloured patches, over the clump identity the
/// generation pass wrote. `crcbl_shaders::grass::patch_of`.
float grass_patch(uint clump)
{
    return grass_unit_pair(grass_hash(clump ^ GRASS_PATCH_SALT)).x;
}

/// A row by the index an instance names, clamped into the table.
GrassBlade grass_row(uint index)
{
    return blades[min(index, max(grass.limits.y, 1u) - 1u)];
}

/// Decision 4's levers, over a colour its row has already graded from root to
/// tip: the root's occlusion colour, the patch's, and the additive tip colour.
///
/// `along` is the height up the blade in `0..1` and `patch` the blade's patch
/// lane. **Every lever at zero returns `color` exactly** — a `lerp` by zero and
/// an add of zero — which is what keeps a row that pulls none of them drawing
/// the picture it drew before they existed.
float3 grass_style(float3 color, GrassBlade row, float along, float patch)
{
    float reach = row.occlusion.a;
    float occluded = reach > 0.0 ? saturate(1.0 - along / reach) : 0.0;
    float3 styled = lerp(color, row.occlusion.rgb, occluded);
    styled = lerp(styled, row.patch.rgb, row.patch.a * patch);
    float start = row.glow.a;
    float glow = saturate((along - start) / max(1.0 - start, 1e-4));
    return styled + row.glow.rgb * glow;
}

/// The light reaching a blade at `world_position` shading by `normal`, before
/// its albedo: every clustered light through the copied walks, plus the frame's
/// flat ambient.
///
/// **Lambert, and nothing beside it** — see the header.
float3 grass_light(float3 world_position, float3 normal, float2 pixel)
{
    // The blade's own plane has no bearing on the shading — see the header —
    // but it is what the shadow biases want, and the normal it shades by is the
    // nearest thing to it a cutout has.
    float3 geometric_normal = normal;

    // This fragment's froxel and the lights in it, `mesh.slang`'s lookup: the
    // view depth out of the matrix rather than off `SV_Position.w`, which the
    // four targets have twice disagreed about.
    float view_depth = mul(frame.view_proj, float4(world_position, 1.0)).w;
    uint base = froxel_of(pixel, view_depth) * CLUSTER_STRIDE;
    uint count = min(cluster_lights[base], CLUSTER_LIGHT_CAPACITY);

    float3 direct = float3(0.0, 0.0, 0.0);
    for (uint slot = 0; slot < count; ++slot)
    {
        GpuLight light = lights[cluster_lights[base + 1 + slot]];
        if (light.kind == KIND_RECT)
        {
            // Skipped rather than shaded as something else: a rectangle's
            // contribution is a polygon integral, and a row read as a point at
            // its own centre draws a plausible highlight that is not the light.
            continue;
        }

        float3 to_light;
        float reach;
        if (light.kind == KIND_DIRECTIONAL)
        {
            to_light = normalize(light.direction.xyz);
            reach = 1.0;
        }
        else
        {
            float3 offset = light.position.xyz - world_position;
            float distance = length(offset);
            to_light = offset / max(distance, 1e-6);
            reach = punctual_falloff(distance, light.position.w);
            if (light.kind == KIND_SPOT)
            {
                reach *= spot_cone(to_light, light.direction.xyz, light.direction.w,
                                   light.cos_inner);
            }
        }

        float n_dot_l = dot(normal, to_light);
        if (light.kind == KIND_DIRECTIONAL)
        {
            uint sun_cascade;
            float sun_fade;
            reach = sun_visibility(world_position, to_light, n_dot_l, geometric_normal, pixel,
                                   sun_cascade, sun_fade);
        }
        else if (light.kind == KIND_POINT)
        {
            if (light.shadow_tile <= SHADOW_LIGHT_TILES - SHADOW_POINT_FACES)
            {
                reach *= point_visibility(light, light.shadow_tile, world_position, to_light,
                                          n_dot_l, geometric_normal, pixel);
            }
        }
        else if (light.shadow_tile < SHADOW_LIGHT_TILES)
        {
            reach *= spot_visibility(light, light.shadow_tile, world_position, to_light, n_dot_l,
                                     geometric_normal, pixel);
        }

        direct += light.color.rgb * (max(n_dot_l, 0.0) * reach);
    }

    return direct + frame.ambient.rgb;
}

/// What the vertex stage hands the fragment stage.
struct GrassVertex
{
    float4 position : SV_Position;
    /// The world-space point, for the shadow walk and the light grid.
    float3 world_position : WORLD_POSITION;
    /// The normal the card shades by: the ground's under the blade's root —
    /// decision 4's "normals taken from the ground … so a field shades as one
    /// surface" — or straight up, where its row asks for that.
    float3 normal : NORMAL;
    /// The blade's colour at this point: its row's root-to-tip gradient, tinted
    /// by its own lane.
    float3 color : COLOR;
    /// `x` across the card and `y` up it, zero at the root.
    float2 uv : TEXCOORD0;
    /// Which level of the card's chain this blade reads — see
    /// `grassCardLevel`. Flat across the card by construction: every vertex of
    /// one instance computes it from the same width and the same depth.
    float level : CARD_LEVEL;
    /// The blade's row, for the levers the fragment stage pulls.
    nointerpolation uint row : BLADE_ROW;
    /// The blade's patch lane — see `grass_patch`.
    nointerpolation float patch : PATCH;
};

/// A mesh blade's four control points, wind included.
struct GrassBladeCurve
{
    float3 p0;
    float3 p1;
    float3 p2;
    float3 p3;
};

/// The Bézier `blade` of row `row` is drawn along.
///
/// **No angle anywhere.** The tip stands `tilt` of the blade's height out along
/// its face and the rest of the height up — `sqrt(1 - tilt²)`, God of War's
/// restored length — so the chord is the blade's height long whatever the tilt.
/// The two middle points stand at thirds of the chord, bowed back from it by
/// `bow` of the height along the unit vector perpendicular to the chord in the
/// blade's plane. The lean is the card's `t²` curve in Bernstein form — nothing
/// on the first two points, a third on the third and all of it on the tip — so
/// calm is still by the multiply, `grass_lean`'s way.
GrassBladeCurve grass_blade_curve(GrassInstance blade, GrassBlade row)
{
    float height = blade.root.w;
    float3 face = float3(blade.facing.x, 0.0, blade.facing.y);
    float tilt = row.shape.x;
    float upright = sqrt(max(1.0 - tilt * tilt, 0.0));
    float3 chord = float3(0.0, height * upright, 0.0) + face * (height * tilt);
    // `cross(chord, side) / height`, written out: unit, in the blade's plane,
    // at right angles to the chord and leaning toward the face.
    float3 across = float3(face.x * upright, -tilt, face.z * upright);
    float3 bow = across * (row.shape.y * height);

    GrassBladeCurve curve;
    curve.p0 = blade.root.xyz;
    curve.p1 = curve.p0 + chord * (1.0 / 3.0) - bow;
    curve.p2 = curve.p0 + chord * (2.0 / 3.0) - bow + blade.lean.xyz * (1.0 / 3.0);
    curve.p3 = curve.p0 + chord + blade.lean.xyz;
    return curve;
}

/// One vertex of a blade, before the level of detail mixes it with another.
struct GrassBladePoint
{
    float3 position;
    float3 normal;
};

/// The vertex of `curve` at `t` up it, on edge `edge` — minus one, plus one,
/// or zero at the tip — with its width scaled by `scale`.
///
/// **The width's direction turns toward the view as the blade turns edge-on**:
/// the blade's own side where it is seen face-on, and the direction at right
/// angles to both the curve and the view where it is seen along its side, in
/// the proportion its side is seen. The half-width is then floored so what is
/// seen of it is never under half of `GRASS_BLADE_MIN_PIXELS` at this depth —
/// and only then scaled, so a blade the near level is narrowing away still
/// narrows to nothing.
GrassBladePoint grass_blade_point(GrassBladeCurve curve, GrassInstance blade, GrassBlade row,
                                  float t, float edge, float scale)
{
    float u = 1.0 - t;
    float3 centre = curve.p0 * (u * u * u) + curve.p1 * (3.0 * u * u * t)
                  + curve.p2 * (3.0 * u * t * t) + curve.p3 * (t * t * t);
    float3 tangent = (curve.p1 - curve.p0) * (3.0 * u * u) + (curve.p2 - curve.p1) * (6.0 * u * t)
                   + (curve.p3 - curve.p2) * (3.0 * t * t);
    float3 side = float3(-blade.facing.y, 0.0, blade.facing.x);

    float3 to_eye = frame.camera_position.xyz - centre;
    float3 view = to_eye / max(length(to_eye), 1e-6);
    float3 perpendicular = cross(tangent, view);
    float perpendicular_length = length(perpendicular);
    float3 screen_side = perpendicular_length > 1e-6 ? perpendicular / perpendicular_length : side;
    screen_side = dot(screen_side, side) < 0.0 ? -screen_side : screen_side;
    float along = dot(side, view);
    float side_seen = sqrt(saturate(1.0 - along * along));
    float3 direction = side * side_seen + screen_side * (1.0 - side_seen);
    float seen = length(direction - view * dot(direction, view));

    // Metres one pixel spans at this depth — `grass_sheet_vertex`'s footprint.
    float pixel = max(abs(mul(frame.view_proj, float4(centre, 1.0)).w), 1e-4)
                / max(grass.screen.x, 1e-4);
    float half_width = blade.facing.z * (1.0 - t * t);
    float least = 0.5 * GRASS_BLADE_MIN_PIXELS * pixel;
    float widened = half_width * seen < least ? least / max(seen, 1e-4) : half_width;

    GrassBladePoint here;
    here.position = centre + direction * (edge * widened * scale);
    // The derivative crossed with the side, which faces the blade's face, and
    // tilted out toward this edge by the row's rounding.
    float3 normal = cross(tangent, side);
    normal = normal / max(length(normal), 1e-6);
    here.normal = normalize(normal + side * (edge * row.shape.z));
    return here;
}

/// How far into the band before the level switch a blade at `distance` metres
/// is: `x` its drop, over the band's first half, and `y` its morph, over the
/// second. Both exactly zero outside the band's near end and for a band of
/// zero, which is a hard switch.
float2 grass_blade_lod(float distance)
{
    float band = field.blades.y;
    if (!(band > 0.0))
    {
        return float2(0.0, 0.0);
    }
    float half_band = 0.5 * band;
    float start = field.blades.x - band;
    return float2(saturate((distance - start) / half_band),
                  saturate((distance - start - half_band) / half_band));
}

/// A blade's vertex: `index` of a strip of `segments` pairs and a tip, at the
/// level of detail `lod` — its drop and its morph — for `blade`.
///
/// **What the morph mixes toward is the far strip itself.** A near pair at
/// `k / 7` of the height lies inside exactly one far segment — seven and three
/// share no factor — and is mixed toward the point that far segment's straight
/// line has at that height, built from the far strip's own two vertices by the
/// same `grass_blade_point`. The mix is written `a * (1 - m) + b * m`, which at
/// a morph of one is `b` exactly.
GrassVertex grass_blade_vertex(GrassInstance blade, uint index, uint segments, float2 lod)
{
    GrassBlade row = grass_row(blade.lanes.y);
    GrassBladeCurve curve = grass_blade_curve(blade, row);
    bool kept = blade.lanes.w != 0u;
    float drop = lod.x;
    float morph = lod.y;
    // The kept blade widens toward the far level's width as the three around it
    // narrow away, so the band's coverage does not dip.
    float scale = kept ? 1.0 + (GRASS_BLADE_FAR_WIDEN - 1.0) * drop : 1.0 - drop;

    bool tip = index >= 2u * segments;
    uint pair = min(index / 2u, segments);
    float edge = tip ? 0.0 : (index % 2u == 0u ? -1.0 : 1.0);
    float t = tip ? 1.0 : float(pair) / float(segments);
    GrassBladePoint here = grass_blade_point(curve, blade, row, t, edge, scale);

    if (morph > 0.0 && !tip && segments != GRASS_BLADE_FAR_SEGMENTS)
    {
        uint scaled = pair * GRASS_BLADE_FAR_SEGMENTS;
        uint lower = scaled / segments;
        float within = float(scaled - lower * segments) / float(segments);
        GrassBladePoint a = grass_blade_point(
            curve, blade, row, float(lower) / float(GRASS_BLADE_FAR_SEGMENTS), edge, scale);
        GrassBladePoint b = grass_blade_point(
            curve, blade, row, float(lower + 1u) / float(GRASS_BLADE_FAR_SEGMENTS), edge, scale);
        float3 far_position = a.position * (1.0 - within) + b.position * within;
        float3 far_normal = a.normal * (1.0 - within) + b.normal * within;
        here.position = here.position * (1.0 - morph) + far_position * morph;
        here.normal = here.normal * (1.0 - morph) + far_normal * morph;
    }

    // Two-sided: the side of the blade the camera sees is the side it shades.
    float3 to_eye = frame.camera_position.xyz - here.position;
    float3 blade_normal = dot(here.normal, to_eye) < 0.0 ? -here.normal : here.normal;
    // Decision 3's "far blades blend toward the clump normal": the row's own
    // normal lever — the ground's or straight up — leaning out from the clump's
    // point, so a far clump shades as a tuft.
    float3 base = row.flags.y == GRASS_NORMAL_UP ? float3(0.0, 1.0, 0.0) : blade.ground.xyz;
    float radius = max(float(GRASS_CLUMP_CELLS) * field.origin.w, 1e-4);
    float3 clump_normal =
        normalize(base + float3(blade.clump.x, 0.0, blade.clump.y) * (GRASS_CLUMP_DOME / radius));

    GrassVertex output;
    output.position = mul(frame.view_proj, float4(here.position, 1.0));
    output.world_position = here.position;
    output.normal = normalize(normalize(blade_normal) * (1.0 - morph) + clump_normal * morph);
    float tint = 1.0 - GRASS_TINT_RANGE * blade.facing.w;
    output.color = lerp(row.root_color.rgb, row.tip_color.rgb, t) * tint;
    output.uv = float2(0.5 + 0.5 * edge, t);
    output.level = 0.0;
    output.row = min(blade.lanes.y, max(grass.limits.y, 1u) - 1u);
    output.patch = grass_patch(blade.lanes.z);
    return output;
}

/// Two crossed quads per instance, built from `SV_VertexID`.
[shader("vertex")]
GrassVertex vertexMain(uint index: SV_VertexID, uint instance_id: SV_InstanceID)
{
    // The slot's run, reached by the base this adds rather than by the draw's
    // `firstInstance` — see the header.
    GrassInstance blade = instances[tile.slot.x * grass.limits.x + instance_id];
    GrassBlade row = grass_row(blade.lanes.y);

    uint quad = index / GRASS_CARD_QUAD_VERTICES;
    float2 corner = GRASS_CARD_CORNERS[index % GRASS_CARD_QUAD_VERTICES];

    // The second quad is the first turned a quarter turn about `+Y`, which for
    // a horizontal unit vector is the swap and one negation below — the
    // rotation decision 9 asks for, written without an angle.
    float2 facing = blade.facing.xy;
    float2 across = quad == 0u ? facing : float2(-facing.y, facing.x);

    float3 sideways = float3(across.x, 0.0, across.y) * (blade.facing.z * (corner.x * 2.0 - 1.0));
    // The lean rises as the square of the height along the blade, so the root
    // does not move and the tip carries the whole of it: the curve a stalk
    // rooted in the ground bends in, and the cheapest one that is flat at the
    // root.
    float3 leaning = blade.lean.xyz * (corner.y * corner.y);
    float3 world = blade.root.xyz + sideways + float3(0.0, blade.root.w * corner.y, 0.0) + leaning;

    GrassVertex output;
    output.position = mul(frame.view_proj, float4(world, 1.0));
    output.world_position = world;
    output.normal = row.flags.y == GRASS_NORMAL_UP ? float3(0.0, 1.0, 0.0) : blade.ground.xyz;
    // The row's root-to-tip gradient — decision 4's first lever — scaled by the
    // blade's own tint lane over `GRASS_TINT_RANGE`, so a field is not one flat
    // colour and no blade is darker than that fraction of its row.
    float tint = 1.0 - GRASS_TINT_RANGE * blade.facing.w;
    output.color = lerp(row.root_color.rgb, row.tip_color.rgb, corner.y) * tint;
    output.uv = corner;
    // The card's width on screen, from its own width in metres and the depth
    // this vertex came out at. `w` is the view depth for a perspective
    // projection and one for an orthographic one, which is the right answer in
    // both cases: an orthographic camera does not shrink a card with distance.
    output.level = grassCardLevel(2.0 * blade.facing.z * grass.screen.x
                                  / max(abs(output.position.w), 1e-4));
    output.row = min(blade.lanes.y, max(grass.limits.y, 1u) - 1u);
    output.patch = grass_patch(blade.lanes.z);
    return output;
}

/// A near mesh blade: fifteen vertices, dropping and morphing toward the far
/// level in the band before the switch.
[shader("vertex")]
GrassVertex bladeNearVertexMain(uint index: SV_VertexID, uint instance_id: SV_InstanceID)
{
    // The slot's near run is in the second region — see the instances binding.
    uint slots = field.tiles.x * field.tiles.y;
    GrassInstance blade = instances[(slots + tile.slot.x) * grass.limits.x + instance_id];
    float distance = length(blade.root.xyz - frame.camera_position.xyz);
    return grass_blade_vertex(blade, index, GRASS_BLADE_NEAR_SEGMENTS, grass_blade_lod(distance));
}

/// A far mesh blade: seven vertices, every one of them a blade the far level
/// keeps, at its full far width.
[shader("vertex")]
GrassVertex bladeFarVertexMain(uint index: SV_VertexID, uint instance_id: SV_InstanceID)
{
    // Counted down from the end of the slot's first region.
    uint capacity = grass.limits.x;
    GrassInstance blade = instances[tile.slot.x * capacity + capacity - 1u - instance_id];
    return grass_blade_vertex(blade, index, GRASS_BLADE_FAR_SEGMENTS, float2(1.0, 1.0));
}

/// A mesh blade, styled and lit. No cutout: the strip is the blade.
[shader("fragment")]
float4 bladeFragmentMain(GrassVertex input) : SV_Target
{
    float3 color = grass_style(input.color, blades[input.row], input.uv.y, input.patch);
    float3 normal = normalize(input.normal);
    return float4(color * grass_light(input.world_position, normal, input.position.xy), 1.0);
}

/// The card, cut out, styled and lit.
[shader("fragment")]
float4 fragmentMain(GrassVertex input) : SV_Target
{
    // The cook's own level, chosen for the whole card by the vertex stage —
    // see `grassCardLevel` for why it is not the hardware's.
    float coverage =
        grassCard
            .SampleLevel(grassCardSampler, float3(input.uv.x, 1.0 - input.uv.y, 0.0), input.level)
            .r;
    if (coverage < GRASS_ALPHA_CUTOFF)
    {
        discard;
    }

    float3 color = grass_style(input.color, blades[input.row], input.uv.y, input.patch);
    // Interpolation denormalises, `mesh.slang`'s reason exactly.
    float3 normal = normalize(input.normal);
    return float4(color * grass_light(input.world_position, normal, input.position.xy), 1.0);
}

/// What a shell's or a fin's vertex hands the fragment stage.
struct GrassShellVertex
{
    float4 position : SV_Position;
    /// The world-space point as drawn — bent by the wind — for the light walk.
    float3 world_position : WORLD_POSITION;
    /// The same point **before** the wind moved it: `xz` where on the ground it
    /// stands and `y` its height above the ground in metres. What a strand is
    /// looked up by, so a bent layer carries its strands with it.
    float3 rest : REST_POSITION;
    /// The ground's unit normal under the point.
    float3 normal : NORMAL;
    /// Metres of world one pixel spans at this point's depth.
    float footprint : FOOTPRINT;
    /// The occlusion this height is lit by — a shell's own row of the table, a
    /// fin's nearest one.
    float occlusion : OCCLUSION;
    /// `x` zero for a shell, one for a fin on a line of constant `x` and two for
    /// one of constant `z`; `y` how far the fin has faded in, one for a shell;
    /// `z` the fin line's world coordinate; `w` unread.
    nointerpolation float4 fin : FIN;
};

/// A point of a shell's or a fin's sheet: `rest` on the ground, `share` of the
/// stack's height up it.
GrassShellVertex grass_sheet_vertex(float2 rest, float share, float4 fin, float occlusion)
{
    GrassGround under = grass_ground_under(rest);
    float lift = share * field.stack.x;
    float3 ground = float3(rest.x, under.height, rest.y);
    // Decision 5: "shells displace along the wind in proportion to the shell's
    // height constant". The wind at the ground under this point bends a blade
    // the stack's height through `grass_lean`, and a layer takes the share of
    // that bend a card's point at the same share of its height does — the square
    // of it. **Calm is still by the multiply**, `grass_lean`'s way.
    float3 lean = grass_lean(windSample(ground - frame.camera_position.xyz), field.stack.x);
    float3 world = ground + float3(0.0, lift, 0.0) + lean * (share * share);

    GrassShellVertex output;
    output.position = mul(frame.view_proj, float4(world, 1.0));
    output.world_position = world;
    output.rest = float3(rest.x, lift, rest.y);
    output.normal = under.normal;
    // `grass.screen.x` is how many pixels a metre spans at one unit of view
    // depth, so its reciprocal times this depth is one pixel in metres.
    output.footprint = max(abs(output.position.w), 1e-4) / max(grass.screen.x, 1e-4);
    output.occlusion = occlusion;
    output.fin = fin;
    return output;
}

/// How many shells the field stacks, at least one.
uint grass_shell_count()
{
    return clamp(uint(field.stack.y + 0.5), 1u, GRASS_MAX_SHELLS);
}

/// One shell layer of one tile: `GRASS_SHELL_GRID`² quads lifted off the ground.
[shader("vertex")]
GrassShellVertex shellVertexMain(uint index: SV_VertexID, uint instance_id: SV_InstanceID)
{
    uint shells = grass_shell_count();
    // Counted from the top — see the header.
    uint shell = shells - 1u - min(instance_id, shells - 1u);
    uint quad = index / GRASS_CARD_QUAD_VERTICES;
    float2 corner = GRASS_CARD_CORNERS[index % GRASS_CARD_QUAD_VERTICES];
    float2 cell = float2(float(quad % GRASS_SHELL_GRID), float(quad / GRASS_SHELL_GRID)) + corner;
    float2 rest = tile.tile.xy + cell * (tile.tile.z / float(GRASS_SHELL_GRID));
    float4 layer = field.layers[shell];
    return grass_sheet_vertex(rest, layer.x, float4(0.0, 1.0, 0.0, 0.0), layer.y);
}

/// The occlusion a point `share` of the way up the stack is lit by: the highest
/// shell at or below it, or the lowest shell's below that.
float grass_occlusion_at(float share)
{
    uint shells = grass_shell_count();
    float occlusion = field.layers[0].y;
    for (uint shell = 0u; shell < shells; ++shell)
    {
        if (field.layers[shell].x <= share)
        {
            occlusion = field.layers[shell].y;
        }
    }
    return occlusion;
}

/// One tile's fins: on each axis a line through the centre of every band of
/// `GRASS_FIN_SPACING` cells, each `GRASS_FIN_SEGMENTS` quads along and
/// `GRASS_FIN_ROWS` up.
///
/// **A fin not near a silhouette is collapsed here**, per segment: the ground
/// normals either side of it are read, and a segment both of whose sides face
/// the camera more squarely than `GRASS_FIN_GRAZE_START` is folded onto a point
/// outside the clip volume. Where the two sides face opposite ways — a crest —
/// it stands whatever the angle. Between the two thresholds the fin's strands
/// are narrowed rather than the fin removed, so one does not pop in.
[shader("vertex")]
GrassShellVertex finVertexMain(uint index: SV_VertexID)
{
    uint quad = index / GRASS_CARD_QUAD_VERTICES;
    float2 corner = GRASS_CARD_CORNERS[index % GRASS_CARD_QUAD_VERTICES];
    uint lines = max(field.tiles.z / GRASS_FIN_SPACING, 1u);
    uint strip = GRASS_FIN_SEGMENTS * GRASS_FIN_ROWS;
    uint axis = min(quad / (lines * strip), 1u);
    uint fin_line = (quad / strip) % lines;
    uint segment = (quad / GRASS_FIN_ROWS) % GRASS_FIN_SEGMENTS;
    uint rung = quad % GRASS_FIN_ROWS;

    float band = float(GRASS_FIN_SPACING) * field.origin.w;
    float across = (float(fin_line) + 0.5) * band;
    float stride = tile.tile.z / float(GRASS_FIN_SEGMENTS);
    float along = (float(segment) + corner.x) * stride;
    float middle_along = (float(segment) + 0.5) * stride;
    float share = (float(rung) + corner.y) / float(GRASS_FIN_ROWS);

    // Axis zero is a line of constant `x` running along `z`, axis one a line of
    // constant `z` running along `x`.
    bool along_z = axis == 0u;
    float2 rest = tile.tile.xy + (along_z ? float2(across, along) : float2(along, across));
    float2 middle =
        tile.tile.xy + (along_z ? float2(across, middle_along) : float2(middle_along, across));
    float2 side = along_z ? float2(0.5 * band, 0.0) : float2(0.0, 0.5 * band);

    GrassGround centre = grass_ground_under(middle);
    GrassGround near_side = grass_ground_under(middle - side);
    GrassGround far_side = grass_ground_under(middle + side);
    float3 eye = frame.camera_position.xyz
               - float3(middle.x, centre.height + 0.5 * field.stack.x, middle.y);
    float3 view = eye / max(length(eye), 1e-6);
    float facing_near = dot(near_side.normal, view);
    float facing_far = dot(far_side.normal, view);
    float graze = facing_near * facing_far <= 0.0
                      ? 0.0
                      : min(abs(facing_near), abs(facing_far));
    float fade = saturate((GRASS_FIN_GRAZE_START - graze)
                          / (GRASS_FIN_GRAZE_START - GRASS_FIN_GRAZE_FULL));

    float line_at = (along_z ? tile.tile.x : tile.tile.y) + across;
    GrassShellVertex output = grass_sheet_vertex(
        rest, share, float4(float(axis) + 1.0, fade, line_at, 0.0), grass_occlusion_at(share));
    if (fade <= 0.0 || field.stack.z <= 0.0)
    {
        // Folded onto one point, beyond the far plane of a reversed-Z volume:
        // every triangle of the segment has no area and no fragment.
        output.position = float4(0.0, 0.0, -1.0, 1.0);
    }
    return output;
}

/// A row of zeroes: no blade, which is what an empty cell holds.
GrassInstance grass_no_blade()
{
    GrassInstance none;
    none.root = float4(0.0, 0.0, 0.0, 0.0);
    none.facing = float4(0.0, 0.0, 0.0, 0.0);
    none.lean = float4(0.0, 0.0, 0.0, 0.0);
    none.ground = float4(0.0, 0.0, 0.0, 0.0);
    none.lanes = uint4(0u, 0u, 0u, 0u);
    return none;
}

/// The blade in the cell `global` cells from the field's corner along `+X` and
/// `+Z`, or a zero row — height zero, no strand — outside the field.
GrassInstance grass_cell(int2 global)
{
    int side = int(max(field.tiles.z, 1u));
    int2 extent = int2(int(field.tiles.x), int(field.tiles.y)) * side;
    if (global.x < 0 || global.y < 0 || global.x >= extent.x || global.y >= extent.y)
    {
        return grass_no_blade();
    }
    uint2 at = uint2(global);
    uint cells = uint(side);
    uint slot = (at.y / cells) * field.tiles.x + at.x / cells;
    uint lane = (at.y % cells) * cells + at.x % cells;
    return grassCells[slot * field.tiles.w + lane];
}

/// How far from its root's centre a strand of `blade` reaches at `lift` metres
/// up, or a negative number where no shell strand stands: an empty cell, a card
/// row, or a point above the blade.
float grass_strand_reach(GrassInstance blade, float lift, float floor_reach, float fade)
{
    if (!(lift < blade.root.w) || grass_row(blade.lanes.y).flags.x != GRASS_LOOK_SHELLS)
    {
        return -1.0;
    }
    float bound = 0.5 * field.origin.w;
    float reach = min(blade.facing.z, bound) * (1.0 - lift / blade.root.w);
    return min(max(reach, floor_reach), bound) * fade;
}

/// A shell's or a fin's strand, cut out, styled and lit.
[shader("fragment")]
float4 shellFragmentMain(GrassShellVertex input) : SV_Target
{
    float cell = field.origin.w;
    float2 from_corner = (input.rest.xz - field.origin.xy) / cell;
    int2 home = int2(floor(from_corner));
    float2 inside = from_corner - floor(from_corner);
    int2 toward = int2(inside.x < 0.5 ? -1 : 1, inside.y < 0.5 ? -1 : 1);
    float lift = input.rest.y;
    // No strand narrower than `GRASS_STRAND_MIN_PIXELS` — see that constant's
    // Rust twin for why a cutout needs the floor.
    float floor_reach = 0.5 * GRASS_STRAND_MIN_PIXELS * input.footprint;
    float fade = input.fin.y;

    GrassInstance found = grass_no_blade();
    float nearest = 2.0;
    if (input.fin.x < 0.5)
    {
        // A shell: the four cells nearest the point, and the strand's disc.
        for (int dz = 0; dz < 2; ++dz)
        {
            for (int dx = 0; dx < 2; ++dx)
            {
                GrassInstance blade = grass_cell(home + int2(dx * toward.x, dz * toward.y));
                float reach = grass_strand_reach(blade, lift, floor_reach, fade);
                float2 offset = input.rest.xz - blade.root.xz;
                float apart = sqrt(dot(offset, offset)) / max(reach, 1e-6);
                if (reach > 0.0 && apart < nearest)
                {
                    nearest = apart;
                    found = blade;
                }
            }
        }
    }
    else
    {
        // A fin: every cell of its band across, and the two nearest along it —
        // each strand seen from the side, as the interval its disc covers.
        bool along_z = input.fin.x < 1.5;
        float fin_line = (input.fin.z - (along_z ? field.origin.x : field.origin.y)) / cell;
        int first = int(floor(fin_line + 0.5)) - int(GRASS_FIN_SPACING / 2u);
        int along_home = along_z ? home.y : home.x;
        int along_step = along_z ? toward.y : toward.x;
        for (int band = 0; band < int(GRASS_FIN_SPACING); ++band)
        {
            for (int next = 0; next < 2; ++next)
            {
                int along = along_home + next * along_step;
                GrassInstance blade = grass_cell(along_z ? int2(first + band, along)
                                                         : int2(along, first + band));
                float reach = grass_strand_reach(blade, lift, floor_reach, fade);
                float offset =
                    along_z ? input.rest.z - blade.root.z : input.rest.x - blade.root.x;
                float apart = abs(offset) / max(reach, 1e-6);
                if (reach > 0.0 && apart < nearest)
                {
                    nearest = apart;
                    found = blade;
                }
            }
        }
    }
    if (!(nearest < 1.0))
    {
        discard;
    }

    GrassBlade row = grass_row(found.lanes.y);
    float along_blade = saturate(lift / found.root.w);
    float tint = 1.0 - GRASS_TINT_RANGE * found.facing.w;
    float3 color = lerp(row.root_color.rgb, row.tip_color.rgb, along_blade) * tint;
    color = grass_style(color, row, along_blade, grass_patch(found.lanes.z)) * input.occlusion;
    float3 normal =
        row.flags.y == GRASS_NORMAL_UP ? float3(0.0, 1.0, 0.0) : normalize(input.normal);
    return float4(color * grass_light(input.world_position, normal, input.position.xy), 1.0);
}
