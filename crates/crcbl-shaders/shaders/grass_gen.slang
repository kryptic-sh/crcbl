// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/57-grass.md`'s rungs G1, G2 and G3, first half: one compute
// dispatch per tile, generating the blades the grass pass reads.
//
// # What a dispatch does
//
// Decision 2's lanes: each invocation's id becomes a cell of the tile's grid
// plus a hash jitter, then the density test, then the cover map's blade row and
// the ground's height and slope, then the distance cull, then the wind at the
// root — and what survives is written twice. Once **by its cell**, into
// `grassCells`, at an index the cell alone decides; and once **appended** to one
// of the tile's runs with an atomic add, decision 1's lock-free append, when its
// row is drawn as cards or as mesh blades. A shell row is not appended: its
// slot's shell and fin draws are given their instance counts instead, and the
// shells find the blade by its cell.
//
// # Three runs in two regions, and why none overflows
//
// Slot `s` owns two regions of `instances`, each `capacity` rows. The first is
// `[s * capacity, (s + 1) * capacity)`: **cards counting up from its start and
// far mesh blades counting down from its end**. The second is the same rows past
// `slots * capacity`: **near mesh blades** counting up. Every cell appends at
// most once, to exactly one run, so the cards and far blades between them never
// pass `capacity` and never meet.
//
// # Clumps and the far level, as integers
//
// Decision 2's clumps and decision 3's far level are both decided off the
// cell's place in the whole field, never off a float: `grass_clump_of` is a
// Voronoi search in integer units of a cell, and `grass_kept_far` picks one cell
// of every two-by-two block by the block's hash. `crcbl_shaders::grass`'s
// `clump_of` and `kept_far` are the CPU's copies, the same bits.
//
// # One placement, whatever the look
//
// Decision 3's check — "a field that switches from cards to blades should
// change one description and move nothing else" — is why `grassCells` is
// written for every look. A row's look decides which draw reads a blade and
// nothing about the blade: the same cell, root, size, facing, tint and lean
// land in `grassCells` for a card row and for a shell row, bit for bit.
//
// # Deterministic, and exactly where
//
// **Which blades exist is integer arithmetic throughout.** The cell id is an
// integer, `grass_hash` is integer, the density map is read with `Load` and
// compared as the eight-bit number the texel holds — no filter, no threshold on
// a float — so the set of blades a tile produces is the same set on every
// target and in every run, and `crcbl_render::grass::placement` computes it on
// the CPU from the same three lines.
//
// What is *not* bit-identical between the two copies is a blade's root height
// and its ground normal: those are a bilinear blend of four texels, and a
// shader compiler may contract each multiply-add into an FMA where Rust never
// does. The difference is the last place of an `f32` — the same freedom
// `crcbl_shaders::wind::MAX_CPU_GPU_ERROR` is written for.
//
// # No transcendental
//
// Decision 9's rule, and nothing here needs an escape from it: the jitter, the
// facing, the sizes and the clumps come out of an integer hash, the lean is a
// square root and the wind is `wind.slang`'s own arithmetic, copied below.
//
// # One bind group, and no storage texture
//
// The ground field and the cover map are sampled textures read with `Load`, so
// WebGPU's four storage textures per stage are untouched — and three storage
// buffers, well inside the eight a browser guarantees — four since the shells'
// `grassCells` joined them.

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

/// `wind.slang`'s uniform block, bound here as the very buffer that pass is
/// handed. `crcbl_shaders::grass`'s `the_wind_block_is_the_field_s` is where
/// the two are compared.
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

/// The wind's own three bindings and its sampler, at the numbers `wind.slang`
/// gives them, so the copied sampler below reads the names it was written with.
[[vk::binding(0, 0)]]
ConstantBuffer<WindParams> wind D3D12_REGISTER(b0, space0);

/// The coarse direction layer: a deflection from `baseDirection` per texel, in
/// red and green over `-1..1`. Blue and alpha are unread.
[[vk::binding(1, 0)]]
Texture2D<float4> windDirectionLayer D3D12_REGISTER(t1, space0);

/// The fine intensity layer: a speed multiplier per texel, in red. Zero is
/// calm, and a calm texel is what leaves a blade upright.
[[vk::binding(2, 0)]]
Texture2D<float4> windIntensityLayer D3D12_REGISTER(t2, space0);

/// Linear filtering, `repeat` on both axes — `wind.slang`'s sampler.
[[vk::binding(3, 0)]]
SamplerState windSampler D3D12_REGISTER(s3, space0);

/// What a generation pass needs that is not a buffer's contents, matching
/// `crcbl_shaders::grass::GenParams`.
struct GrassGenParams
{
    /// The world-space camera in `xyz`, and in `w` how far from it a blade is
    /// still generated, in metres — decision 7's distance cull, the only cull
    /// this rung spends.
    float4 camera;
    /// `xy` the world XZ of the ground field's first texel centre, `z` metres
    /// of world per texel and `w` one over that. The reciprocal is carried
    /// rather than computed so the CPU copy and this one multiply by the same
    /// number instead of each dividing by a different rounding of it.
    float4 ground;
    /// The same four numbers for the cover map.
    float4 cover;
    /// `x` texels across the ground field, `y` down it, `z` across the cover
    /// map, `w` down it.
    uint4 maps;
    /// `x` how many instances one tile slot holds, `y` how many rows `blades`
    /// holds, `z` how many cells lie along one side of a tile, and `w` how many
    /// slots there are.
    uint4 limits;
    /// `x` the instance count a slot's shell draw is given once a shell row
    /// grows in it, which is the field's shell count; `y` its fin draw's, one
    /// where the field stands fins and zero where it does not. `zw` are written
    /// as zero and unread.
    uint4 looks;
    /// `x` the distance from the camera, in metres, past which a mesh blade is
    /// appended to the far run rather than the near one. `yzw` are written as
    /// zero and unread.
    float4 lod;
};

[[vk::binding(4, 0)]]
ConstantBuffer<GrassGenParams> grass D3D12_REGISTER(b4, space0);

/// Which tile this dispatch is generating. Bound at a dynamic offset, one row
/// of the ring per tile, which is how "one dispatch per tile" reaches the
/// shader without a push constant — a browser's WGSL has none.
struct GrassTile
{
    /// `xy` the world XZ of the tile's minimum corner, `z` the tile's side in
    /// metres, `w` written as zero and unread.
    float4 tile;
    /// `x` the tile's slot, `y` its index along `+X`, `z` its index along `+Z`,
    /// `w` written as zero and unread.
    uint4 slot;
};

[[vk::binding(5, 0)]]
ConstantBuffer<GrassTile> tile D3D12_REGISTER(b5, space0);

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

[[vk::binding(6, 0)]]
StructuredBuffer<GrassBlade> blades D3D12_REGISTER(t6, space0);

/// The ground the field stands on: one height in metres per texel. Read with
/// `Load`, never sampled — a hardware filter has eight bits of sub-texel
/// precision and the CPU copy of this placement has all of them.
[[vk::binding(7, 0)]]
Texture2D<float> grassGround D3D12_REGISTER(t7, space0);

/// The cover map: density in red and the blade row in green, both as eight-bit
/// numbers. Read with `Load` for the ground field's reason, and compared as
/// integers so the density test is exact.
[[vk::binding(8, 0)]]
Texture2D<float4> grassCover D3D12_REGISTER(t8, space0);

/// Every slot's instances, in two regions of `slots * capacity` rows — see this
/// file's header for which run is where.
[[vk::binding(9, 0)]]
RWStructuredBuffer<GrassInstance> instances D3D12_REGISTER(u9, space0);

/// One non-indexed draw's arguments per slot — vertex count, instance count,
/// first vertex, first instance — which `clearMain` writes and the appends
/// below count into.
///
/// `Atomic<uint>` rather than a plain `uint` with `InterlockedAdd`, for
/// `cull.slang`'s reason: WGSL has no such intrinsic — its atomics are a
/// distinct `atomic<u32>` type — and Slang refuses the entry point outright for
/// that target. Spelled as a type it lowers on all four.
[[vk::binding(10, 0)]]
RWStructuredBuffer<Atomic<uint> > drawArgs D3D12_REGISTER(u10, space0);

/// Every cell's blade, **at the cell's own index** — `[s * capacity + lane]` for
/// lane `lane` of slot `s` — and zero where the cell carries none. What the
/// shells and their fins look a strand up in: a fragment knows where it is, so
/// it knows its cell, and an appended run in scheduler order could not answer
/// it.
[[vk::binding(11, 0)]]
RWStructuredBuffer<GrassInstance> grassCells D3D12_REGISTER(u11, space0);

static const float WIND_MIN_DIRECTION_LENGTH_SQUARED = 1e-12;


/// `wind.slang`'s smoothed triangle wave, copied so the gust this field leans
/// in is the gust every other consumer reads.
float windSmoothTriangle(float u)
{
    float s = abs(frac(u + 0.5) * 2.0 - 1.0);
    return s * s * (3.0 - 2.0 * s);
}

/// `wind.slang`'s sampler, copied body for body — there is no `#include` here,
/// so the guard in `crcbl_shaders::grass` is what keeps the two one formula.
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

    // `D(x)`: the prevailing direction turned by the layer's deflection, as the
    // product of the complex numbers the two stand for, then renormalised.
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

/// Invocations per workgroup, matching `crcbl_shaders::grass::WORKGROUP_SIZE`.
static const uint GRASS_WORKGROUP_SIZE = 64;

/// Words in one non-indexed draw's argument structure: vertex count, instance
/// count, first vertex, first instance. `crcbl_shaders::grass::DRAW_ARG_WORDS`.
static const uint GRASS_DRAW_ARG_WORDS = 4;

/// Draws a slot owns, back to back: cards, shells, fins, near blades, far
/// blades. `crcbl_shaders::grass::DRAWS_PER_SLOT`.
static const uint GRASS_DRAWS_PER_SLOT = 5;

/// Which of a slot's draws the shells and the fins are.
/// `crcbl_shaders::grass::SHELL_DRAW` and `FIN_DRAW`.
static const uint GRASS_SHELL_DRAW = 1;

static const uint GRASS_FIN_DRAW = 2;

/// Which of a slot's draws the two levels of mesh blades are.
/// `crcbl_shaders::grass::BLADE_NEAR_DRAW` and `BLADE_FAR_DRAW`.
static const uint GRASS_BLADE_NEAR_DRAW = 3;

static const uint GRASS_BLADE_FAR_DRAW = 4;

/// Vertices one near and one far mesh blade draw.
/// `crcbl_shaders::grass::BLADE_NEAR_VERTICES` and `BLADE_FAR_VERTICES`.
static const uint GRASS_BLADE_NEAR_VERTICES = 15;

static const uint GRASS_BLADE_FAR_VERTICES = 7;

/// A row drawn as mesh blades. `crcbl_shaders::grass::LOOK_BLADES`.
static const uint GRASS_LOOK_BLADES = 2;

/// Placement cells along one side of a clump's lattice square, and the integer
/// units a cell is divided into for the search.
/// `crcbl_shaders::grass::CLUMP_CELLS` and `CLUMP_UNITS`.
static const uint GRASS_CLUMP_CELLS = 8;

static const uint GRASS_CLUMP_UNITS = 256;

/// The lanes a clump and a far-level block are drawn from.
/// `crcbl_shaders::grass::CLUMP_SALT` and `LOD_SALT`.
static const uint GRASS_CLUMP_SALT = 0x2c1b3c6du;

static const uint GRASS_LOD_SALT = 0x297a2d39u;

/// A row drawn as shells. `crcbl_shaders::grass::LOOK_SHELLS`.
static const uint GRASS_LOOK_SHELLS = 1;

/// Vertices one shell draws. `crcbl_shaders::grass::SHELL_VERTICES`.
static const uint GRASS_SHELL_VERTICES = 1536;

/// Cells across one band of fins, and the quads along and up one fin.
/// `crcbl_shaders::grass::FIN_SPACING`, `FIN_SEGMENTS` and `FIN_ROWS`.
static const uint GRASS_FIN_SPACING = 4;

static const uint GRASS_FIN_SEGMENTS = 16;

static const uint GRASS_FIN_ROWS = 4;

/// Vertices one card instance draws: two crossed quads of six.
/// `crcbl_shaders::grass::CARD_VERTICES`.
static const uint GRASS_CARD_VERTICES = 12;

/// The lane `grass_hash` is asked for the density test on, exclusive-ored into
/// the cell so the accept/reject is independent of the jitter the same cell's
/// first hash produced.
static const uint GRASS_DENSITY_SALT = 0x9e3779b9u;

/// The lane the facing is drawn from.
static const uint GRASS_FACING_SALT = 0x85ebca6bu;

/// The lane the per-blade height and width are drawn from.
static const uint GRASS_SIZE_SALT = 0xc2b2ae35u;

/// The lane the per-blade tint is drawn from. A lane of its own rather than the
/// spare half of another: a tint that was the same bits as the jitter would
/// paint every blade at the front of its cell one colour.
static const uint GRASS_TINT_SALT = 0x27d4eb2fu;

/// The most of its own height a blade's tip is moved by the wind.
/// `crcbl_shaders::grass::MAX_BEND`.
static const float GRASS_MAX_BEND = 0.6;

/// The wind speed, in m/s, at which a blade has given half of `GRASS_MAX_BEND`.
/// Between God of War's *breezy* and *strong* — `crcbl_wind::Beaufort` — so a
/// named preset lands on either side of it.
/// `crcbl_shaders::grass::BEND_HALF_SPEED`.
static const float GRASS_BEND_HALF_SPEED = 6.0;

/// Jarzynski and Olano's `pcg` hash (*Hash Functions for GPU Rendering*,
/// JCGT 9(3), 2020): PCG's 32-bit LCG step followed by its RXS-M-XS output
/// permutation.
///
/// **Integer throughout, which is the whole of the determinism argument.** Every
/// operation here is exact on every target — a `u32` multiply wraps the same way
/// in Slang, in WGSL, in MSL, in DXIL and in Rust — so the lanes a cell is given
/// are the same bits everywhere, and `crcbl_render::grass::placement::hash` is
/// the same five lines. The constants are PCG's own: `747796405` is the
/// multiplier of O'Neill's 32-bit LCG and `2891336453` its increment.
uint grass_hash(uint value)
{
    uint state = value * 747796405u + 2891336453u;
    uint word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

/// The top sixteen bits of `lane` as a number in `0..1`, and the bottom
/// sixteen as a second one.
///
/// Sixteen bits rather than the whole word because `float(uint)` is exact only
/// below `2^24`, and a jitter that rounded would put two cells at one position
/// on one target and not on another.
float2 grass_unit_pair(uint lane)
{
    return float2(float(lane & 0xffffu), float((lane >> 16u) & 0xffffu)) * (1.0 / 65536.0);
}

/// How far the wind `velocity` moves the tip of a blade `height` metres tall: its
/// travel along the wind in `xz` and its drop in `y`.
///
/// Decision 5's lean, and **the one formula every look bends by** — the card's
/// tip here, and a shell's layer in `grass.slang`, which copies this body. Calm
/// is still by the multiply: a zero velocity makes `bend` exactly zero, so the
/// travel is `0 * (0 / 1e-6)` and the drop is `0 / (2h)`.
float3 grass_lean(float3 velocity, float height)
{
    float speed = length(velocity);
    float bend = height * GRASS_MAX_BEND * (speed / (speed + GRASS_BEND_HALF_SPEED));
    float2 along = velocity.xz * (bend / max(speed, 1e-6));
    // God of War's "translate and restore length": the tip has travelled `bend`
    // sideways, so it drops by `h - sqrt(h² - bend²)`. Written as the
    // difference of squares over the sum, which is the same number without the
    // cancellation — and which is exactly zero when `bend` is, where the
    // subtraction would leave a rounding of `sqrt(h²)` behind and make a calm
    // blade lean by a tenth of a micron.
    float drop = bend * bend / (height + sqrt(max(height * height - bend * bend, 0.0)));
    return float3(along.x, -drop, along.y);
}

/// The height and the ground's unit normal under a world XZ position.
struct GrassGround
{
    /// Metres above the origin.
    float height;
    /// The ground's unit normal.
    float3 normal;
};

/// One texel of the ground field, clamped to its edge.
float grass_ground_texel(int2 texel)
{
    int2 last = int2(int(grass.maps.x) - 1, int(grass.maps.y) - 1);
    return grassGround.Load(int3(clamp(texel, int2(0, 0), last), 0));
}

/// The ground under `world`, blended bilinearly from the four texels around it
/// — and its slope from the same four, so a blade's height and the normal it
/// shades by cost one fetch of the field between them.
GrassGround grass_ground_under(float2 world)
{
    float2 at = (world - grass.ground.xy) * grass.ground.w;
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
    // The cell's own slope, in metres of rise per texel, from the four heights
    // already read. A central difference over four more fetches would be
    // smoother and costs four times the bandwidth for a normal a card is shaded
    // flat by.
    float rise_x = 0.5 * ((h10 - h00) + (h11 - h01));
    float rise_z = 0.5 * ((h01 - h00) + (h11 - h10));
    under.normal = normalize(float3(-rise_x, grass.ground.z, -rise_z));
    return under;
}

/// The cover map's texel under `world`: density in red and the blade row in
/// green. Nearest, never blended — the density test below is an integer
/// comparison and a blend would make it a float one.
float4 grass_cover_under(float2 world)
{
    float2 at = (world - grass.cover.xy) * grass.cover.w;
    int2 last = int2(int(grass.maps.z) - 1, int(grass.maps.w) - 1);
    int2 texel = clamp(int2(floor(at + 0.5)), int2(0, 0), last);
    return grassCover.Load(int3(texel, 0));
}

/// The key a lattice square at `(x, z)` hashes from.
/// `crcbl_shaders::grass::lattice_key`.
uint grass_lattice_key(int2 square)
{
    return uint(square.x) * 0x8da6b343u ^ uint(square.y) * 0xd8163841u;
}

/// The clump a blade belongs to: its identity, and where the blade stands from
/// the clump's point in `GRASS_CLUMP_UNITS` of a cell.
struct GrassClump
{
    uint id;
    int2 offset;
};

/// Decision 2's clumps: the nearest of the nine hash-jittered points around the
/// blade of cell `global`, whose jitter lane is `jitter`.
///
/// **Integer throughout** — `crcbl_shaders::grass::clump_of` is the same loop
/// and the same bits. The squared distances stay under `2^31`: a point is never
/// more than two squares of `GRASS_CLUMP_CELLS * GRASS_CLUMP_UNITS` away on an
/// axis.
GrassClump grass_clump_of(uint2 global, uint jitter)
{
    int units = int(GRASS_CLUMP_UNITS);
    int span = int(GRASS_CLUMP_CELLS * GRASS_CLUMP_UNITS);
    int2 at = int2(global) * units
            + int2(int((jitter & 0xffffu) >> 8u), int(((jitter >> 16u) & 0xffffu) >> 8u));
    int2 home = int2(global / GRASS_CLUMP_CELLS);
    GrassClump nearest;
    nearest.id = 0u;
    nearest.offset = int2(0, 0);
    int best = 0x7fffffff;
    for (int dz = -1; dz <= 1; ++dz)
    {
        for (int dx = -1; dx <= 1; ++dx)
        {
            int2 square = home + int2(dx, dz);
            uint id = grass_hash(grass_lattice_key(square) ^ GRASS_CLUMP_SALT);
            int2 seed = square * span
                      + int2(int(id & uint(span - 1)), int((id >> 16u) & uint(span - 1)));
            int2 offset = at - seed;
            int distance = offset.x * offset.x + offset.y * offset.y;
            if (distance < best)
            {
                best = distance;
                nearest.id = id;
                nearest.offset = offset;
            }
        }
    }
    return nearest;
}

/// Whether the far level keeps the blade of cell `global`: one cell of every
/// two-by-two block, by the block's hash. `crcbl_shaders::grass::kept_far`.
bool grass_kept_far(uint2 global)
{
    uint pick = grass_hash(grass_lattice_key(int2(global >> 1u)) ^ GRASS_LOD_SALT) & 3u;
    return (global.x & 1u) + 2u * (global.y & 1u) == pick;
}

/// An eight-bit unorm texel back as the integer it holds.
///
/// `n / 255` is not exactly representable for most `n`, so the round trip needs
/// the rounding; with it, every one of the 256 values comes back exactly.
uint grass_unorm8(float value)
{
    return uint(value * 255.0 + 0.5);
}

/// One invocation per slot: the arguments of the draw that slot's blades are
/// read by, with the instance count at zero for the appends to count into.
///
/// **A pass of its own, and it has to be.** The appends below are an atomic add
/// on the very word this writes, and the graph emits its barriers between
/// passes rather than inside one — `draw_gen.slang`'s `clear-counters` is the
/// same arrangement for the same reason.
[shader("compute")]
[numthreads(64, 1, 1)]
void clearMain(uint3 thread: SV_DispatchThreadID)
{
    uint slot = thread.x;
    if (slot >= grass.limits.w)
    {
        return;
    }
    // The draws a slot owns, each at an instance count of zero: the cards and
    // both levels of blades count up from it, and the shells and fins are set
    // off it only where a shell row grows.
    uint side = grass.limits.z;
    uint fin_vertices = 2u * (side / GRASS_FIN_SPACING) * GRASS_FIN_SEGMENTS * GRASS_FIN_ROWS * 6u;
    for (uint draw = 0u; draw < GRASS_DRAWS_PER_SLOT; ++draw)
    {
        uint at = (slot * GRASS_DRAWS_PER_SLOT + draw) * GRASS_DRAW_ARG_WORDS;
        uint vertices = draw == GRASS_SHELL_DRAW      ? GRASS_SHELL_VERTICES
                      : draw == GRASS_FIN_DRAW        ? fin_vertices
                      : draw == GRASS_BLADE_NEAR_DRAW ? GRASS_BLADE_NEAR_VERTICES
                      : draw == GRASS_BLADE_FAR_DRAW  ? GRASS_BLADE_FAR_VERTICES
                                                      : GRASS_CARD_VERTICES;
        drawArgs[at].store(vertices);
        drawArgs[at + 1u].store(0u);
        drawArgs[at + 2u].store(0u);
        // **Zero, and WebGPU will not take anything else**: `firstInstance` must
        // be zero unless `indirect-first-instance` is enabled, so a slot's run
        // is reached by the base the vertex stage adds rather than by this word.
        drawArgs[at + 3u].store(0u);
    }
}

/// One invocation per cell of one tile.
[shader("compute")]
[numthreads(64, 1, 1)]
void generateMain(uint3 thread: SV_DispatchThreadID)
{
    uint cells = grass.limits.z;
    uint capacity = grass.limits.x;
    // **The append cannot overflow the slot, and this is why.** A slot holds
    // `capacity` instances and each surviving lane appends exactly one, so
    // bounding the lanes by the capacity bounds the count by it — no clamp on
    // the atomic's result, and no slot ever written past its end.
    uint lanes = min(cells * cells, capacity);
    if (thread.x >= lanes)
    {
        return;
    }

    uint lane = thread.x;
    uint cell = tile.slot.x * capacity + lane;
    // **Empty first**, so a cell that carries no blade this frame — bare, or
    // out of reach — reads as zero rather than as whatever the last frame left.
    // A height of zero is what a shell reads as "no strand here".
    GrassInstance empty;
    empty.root = float4(0.0, 0.0, 0.0, 0.0);
    empty.facing = float4(0.0, 0.0, 0.0, 0.0);
    empty.lean = float4(0.0, 0.0, 0.0, 0.0);
    empty.ground = float4(0.0, 0.0, 0.0, 0.0);
    empty.clump = float4(0.0, 0.0, 0.0, 0.0);
    empty.lanes = uint4(0u, 0u, 0u, 0u);
    grassCells[cell] = empty;
    float side = tile.tile.z / float(cells);
    uint jitter_lane = grass_hash(cell);
    float2 jitter = grass_unit_pair(jitter_lane);
    float2 world = tile.tile.xy
                 + (float2(float(lane % cells), float(lane / cells)) + jitter) * side;

    // Decision 2's density, as an integer. The texel is a count out of 255 and
    // the lane is one out of 65536, so `255 * 257 == 65535` is what makes a
    // full-density texel keep every cell but one in 65536 and a zero-density
    // texel keep none of them — both exactly, on every target.
    float4 cover = grass_cover_under(world);
    if ((grass_hash(cell ^ GRASS_DENSITY_SALT) & 0xffffu) >= grass_unorm8(cover.r) * 257u)
    {
        return;
    }

    uint row = min(grass_unorm8(cover.g), max(grass.limits.y, 1u) - 1u);
    GrassBlade blade = blades[row];

    GrassGround under = grass_ground_under(world);
    float3 root = float3(world.x, under.height, world.y);

    // Decision 7's distance cull, by the root rather than by the tip: a blade
    // whose root is out of reach is out of reach.
    float3 relative = root - grass.camera.xyz;
    if (dot(relative, relative) > grass.camera.w * grass.camera.w)
    {
        return;
    }

    // The cell's place in the whole field, which is what its clump and its far
    // level are decided off — integers, so the CPU's copy is the same answer.
    uint2 global = uint2(tile.slot.y * cells + lane % cells, tile.slot.z * cells + lane / cells);
    GrassClump clump = grass_clump_of(global, jitter_lane);

    // The blade's own size, from one more lane. Each is a fraction *taken away*
    // from the row's, so a row's numbers are the tallest and widest it draws —
    // and the clump takes its own share of the height, a lane every blade of the
    // clump shares. **A clump share of zero multiplies by exactly one.**
    float2 spread = grass_unit_pair(grass_hash(cell ^ GRASS_SIZE_SALT));
    float clump_height = grass_unit_pair(grass_hash(clump.id ^ GRASS_SIZE_SALT)).x;
    float height = blade.size.x * (1.0 - blade.size.z * spread.x)
                 * (1.0 - blade.clump.y * clump_height);
    float half_width = blade.size.y * (1.0 - blade.size.w * spread.y);

    // The facing, as a normalised point of the square rather than an angle —
    // decision 9's last line, and it costs neither a transcendental nor a
    // table. The square is not the circle, so a diagonal is about `sqrt(2)`
    // times as likely as an axis; in a field of crossed cards that is not
    // something an eye can find, and the uniform version is a rejection loop.
    //
    // The clump's shared facing is a point of the square drawn the same way,
    // and the row's share of it moves the blade's point toward it **before**
    // the one normalisation — so a share of zero leaves the point, and the
    // facing, exactly the bits they were.
    float2 own = grass_unit_pair(grass_hash(cell ^ GRASS_FACING_SALT)) * 2.0 - 1.0;
    float2 clump_square = grass_unit_pair(grass_hash(clump.id ^ GRASS_FACING_SALT)) * 2.0 - 1.0;
    float2 square = own + (clump_square - own) * blade.clump.x;
    float square_length = dot(square, square);
    float2 facing = square_length > 1e-8 ? square / sqrt(square_length) : float2(1.0, 0.0);

    // Decision 5's lean: the wind at the blade's root, sampled through
    // `wind.slang`'s own formula and bent by `grass_lean`.
    float3 lean = grass_lean(windSample(relative), height);

    GrassInstance instance;
    instance.root = float4(root, height);
    instance.facing =
        float4(facing, half_width, grass_unit_pair(grass_hash(cell ^ GRASS_TINT_SALT)).x);
    instance.lean = float4(lean, 0.0);
    instance.ground = float4(under.normal, 0.0);
    // A multiple of a power of two of the cell's side, so the product below is
    // one exact operation on both copies.
    instance.clump = float4(float2(clump.offset) * (side * (1.0 / float(GRASS_CLUMP_UNITS))), 0.0, 0.0);
    bool kept = grass_kept_far(global);
    instance.lanes = uint4(cell, row, clump.id, kept ? 1u : 0u);

    grassCells[cell] = instance;

    uint slot_args = tile.slot.x * GRASS_DRAWS_PER_SLOT;
    if (blade.flags.x == GRASS_LOOK_SHELLS)
    {
        // A shell row is drawn by the slot's shell and fin draws, which read it
        // out of `grassCells`. **A store rather than an add**: every shell
        // blade of the slot writes the same count, so the race between them
        // writes one value however it resolves.
        drawArgs[(slot_args + GRASS_SHELL_DRAW) * GRASS_DRAW_ARG_WORDS + 1u].store(grass.looks.x);
        drawArgs[(slot_args + GRASS_FIN_DRAW) * GRASS_DRAW_ARG_WORDS + 1u].store(grass.looks.y);
        return;
    }

    if (blade.flags.x == GRASS_LOOK_BLADES)
    {
        // Decision 3's two levels, by the root's distance from the camera: the
        // near run up to `lod.x`, and past it the far run — which holds the one
        // blade of every two-by-two block `grass_kept_far` picks and drops the
        // other three, having morphed them away in the band before the switch.
        if (dot(relative, relative) < grass.lod.x * grass.lod.x)
        {
            uint near_at = drawArgs[(slot_args + GRASS_BLADE_NEAR_DRAW) * GRASS_DRAW_ARG_WORDS + 1u]
                               .add(1u);
            instances[(grass.limits.w + tile.slot.x) * capacity + near_at] = instance;
        }
        else if (kept)
        {
            uint far_at = drawArgs[(slot_args + GRASS_BLADE_FAR_DRAW) * GRASS_DRAW_ARG_WORDS + 1u]
                              .add(1u);
            instances[tile.slot.x * capacity + capacity - 1u - far_at] = instance;
        }
        return;
    }

    // Decision 1's lock-free append: the atomic add returns the value the word
    // held, which is this blade's place in the slot. **The order within a
    // slot is not determined** — that is what lock-free means — so a comparison
    // of two runs sorts on `lanes.x` before it compares.
    uint at = drawArgs[slot_args * GRASS_DRAW_ARG_WORDS + 1u].add(1u);
    instances[tile.slot.x * capacity + at] = instance;
}
