// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/57-grass.md`'s rung G1, second half: the grass pass, drawing the
// cards `grass_gen.slang` generated.
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
// the forward pass is a copy — `docs/plan/51-volumetrics.md`'s arrangement,
// which `volumetric.slang` and `water.slang` are already under. What is copied
// here from `mesh.slang`, body for body:
//
// * `FrameUniforms` and `GpuLight`, bound as the very buffers the forward pass
//   is handed, so every field is read at the offset it was written at;
// * the sun's cascade walk, its receiver biases and its filters;
// * the punctual lights' walk — the falloff, the cone, the atlas tile and the
//   cube face.
//
// `crcbl_shaders::grass`'s tests are where each is compared.
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
    /// This slot held `docs/plan/25-lod.md`'s selection numbers until the light
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
    /// `docs/plan/50-irradiance-probes.md`'s scrolling: a level re-centres on
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
    /// `docs/plan/25-lod.md`'s selection numbers, as `draw_gen.slang`'s
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
    /// `docs/plan/45-shadows.md`'s atlas rung: this file used to derive a cell
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
/// the geometry a row is drawn with and not in the row. Rung G1 draws the cards.
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
    /// `x` the blade's cell in the field, which is the lane every hash below is
    /// drawn from and the key a determinism comparison sorts on. `y` the row of
    /// `blades` it was given. `zw` are written as zero and unread.
    uint4 lanes;
};

[[vk::binding(0, 0)]]
ConstantBuffer<FrameUniforms> frame;

[[vk::binding(1, 0)]]
ConstantBuffer<GrassParams> grass;

[[vk::binding(2, 0)]]
ConstantBuffer<GrassTile> tile;

/// Every slot's instances, back to back, as `grass_gen.slang` appended them.
[[vk::binding(3, 0)]]
StructuredBuffer<GrassInstance> instances;

/// The blade rows, indexed by `GrassInstance::lanes.y`.
[[vk::binding(4, 0)]]
StructuredBuffer<GrassBlade> blades;

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
Texture2DArray<float4> grassCard;

[[vk::binding(6, 0)]]
SamplerState grassCardSampler;

/// The shadow atlas and its comparison sampler, as `mesh.slang` binds them.
[[vk::binding(7, 0)]]
DepthTexture2D shadow_atlas;

[[vk::binding(8, 0)]]
SamplerComparisonState shadow_sampler;

/// The light rows, as `mesh.slang` binds them.
[[vk::binding(9, 0)]]
StructuredBuffer<GpuLight> lights;

/// The froxel lists, as `mesh.slang` binds them: a count and then that many
/// indices into `lights`, `CLUSTER_STRIDE` words apart.
[[vk::binding(10, 0)]]
StructuredBuffer<uint> cluster_lights;

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


/// What the vertex stage hands the fragment stage.
struct GrassVertex
{
    float4 position : SV_Position;
    /// The world-space point, for the shadow walk and the light grid.
    float3 world_position : WORLD_POSITION;
    /// The ground's normal under the blade's root, which is the normal a card
    /// shades by — decision 4's "normals taken from the ground … so a field
    /// shades as one surface".
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
};

/// Two crossed quads per instance, built from `SV_VertexID`.
[shader("vertex")]
GrassVertex vertexMain(uint index: SV_VertexID, uint instance_id: SV_InstanceID)
{
    // The slot's run, reached by the base this adds rather than by the draw's
    // `firstInstance` — see the header.
    GrassInstance blade = instances[tile.slot.x * grass.limits.x + instance_id];
    GrassBlade row = blades[min(blade.lanes.y, max(grass.limits.y, 1u) - 1u)];

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
    output.normal = blade.ground.xyz;
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
    return output;
}

/// The card, cut out and lit.
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

    // Interpolation denormalises, `mesh.slang`'s reason exactly.
    float3 normal = normalize(input.normal);
    // The card's own plane has no bearing on the shading — see the header — but
    // it is what the shadow biases want: acne is a property of the triangle
    // that was rasterised into the map, and the triangle here is the card.
    float3 geometric_normal = normal;

    // This fragment's froxel and the lights in it, `mesh.slang`'s lookup: the
    // view depth out of the matrix rather than off `SV_Position.w`, which the
    // four targets have twice disagreed about.
    float view_depth = mul(frame.view_proj, float4(input.world_position, 1.0)).w;
    uint base = froxel_of(input.position.xy, view_depth) * CLUSTER_STRIDE;
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
            float3 offset = light.position.xyz - input.world_position;
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
            reach = sun_visibility(input.world_position, to_light, n_dot_l, geometric_normal,
                                   input.position.xy, sun_cascade, sun_fade);
        }
        else if (light.kind == KIND_POINT)
        {
            if (light.shadow_tile <= SHADOW_LIGHT_TILES - SHADOW_POINT_FACES)
            {
                reach *= point_visibility(light, light.shadow_tile, input.world_position, to_light,
                                          n_dot_l, geometric_normal, input.position.xy);
            }
        }
        else if (light.shadow_tile < SHADOW_LIGHT_TILES)
        {
            reach *= spot_visibility(light, light.shadow_tile, input.world_position, to_light,
                                     n_dot_l, geometric_normal, input.position.xy);
        }

        // Lambert, and nothing beside it — see the header.
        direct += light.color.rgb * (max(n_dot_l, 0.0) * reach);
    }

    return float4(input.color * (direct + frame.ambient.rgb), 1.0);
}
