// crcbl-targets: spirv, wgsl, msl, dxil

// The resolve half of the probe visibility capture: six cube-face tiles of
// distance folded into the one octahedral layer `mesh.slang` reads.
//
// `probe_capture.slang` draws the tiles; this reads one texel out of them per
// direction and writes the two moments `crcbl_shaders::probe_visibility`
// defines — the distance and its square.
//
// # The directions arrive as data, and that is the point
//
// Nothing here computes an octahedral mapping. `crcbl_shaders::probe_visibility`
// owns `oct_decode`, `interior_texel` and therefore `texel_direction`, it is the
// Rust mirror the render tests hold the shader to, and a second transcription
// of it in Slang would be a second thing that can drift from the layout it is
// supposed to agree with. So the host evaluates `texel_direction` for all
// `EXTENT²` texels once — the same table for every probe, because the layout is
// — and hands it in as `directions`. The border rule comes along for free: a
// border texel's entry is the direction of the interior texel it stands for, so
// the two are filled with the same number by construction.
//
// # Picking the face, and reading the tile
//
// The face comes in with the direction, for the same reason the direction does:
// the six 90° frusta partition space by the **major axis**, which is exact
// rather than approximate — at 90° with a square aspect a frustum's edge planes
// are the diagonals `|x| = |z|` and `|y| = |z|`, precisely where the major axis
// changes — and `crcbl_render::probe_capture::face_of` is the one place that
// rule and the `+X, -X, +Y, -Y, +Z, -Z` face order it indexes are written.
//
// The direction is then projected by that face's own matrix, with `w = 0`: for
// a rigid view matrix the clip `xy` and `w` of a direction and of the point one
// unit along it from the probe are the same numbers, so the tile coordinate
// needs no probe position. Only clip `z` differs, and nothing here reads it.
//
// # A point sample, and what that costs
//
// One texel of the tile, nearest, at the direction's own place in it. That
// makes the second moment exactly the square of the first, which is the
// distribution the ray cast this replaced also produced — the variance a
// Chebyshev bound needs comes from `probe_moments`' bilinear blend of four
// texels, not from within one. The error against an exact ray is at most the
// angular half-texel of a cube face, which is why the face is rendered at
// several times the octahedral resolution; `crcbl_render::probe_capture::FACE`
// carries the arithmetic.

/// Everything the resolve needs that is not a buffer or an image.
struct OctahedralParams
{
    /// Probes in **this dispatch**, which is one chunk of the capture.
    uint probes;
    /// The probe index chunk-local probe 0 is, so the write lands in the right
    /// layer of a moments buffer that spans every probe.
    uint probe_base;
    /// Texels along one side of a whole layer, border included —
    /// `crcbl_shaders::probe_visibility::EXTENT`.
    uint extent;
    /// Texels along one side of one cube-face tile.
    uint face_texels;
    /// Tiles across the distance atlas.
    uint atlas_columns;
    /// Floats between two rows of one layer of the moments buffer. The copy
    /// into the image wants rows on the device's own alignment, so this is not
    /// `extent * 2`.
    uint row_floats;
    /// Floats between two layers of the moments buffer.
    uint layer_floats;
    /// Padding to a `float4` boundary; unread.
    uint reserved;
};

[[vk::binding(0, 0)]]
ConstantBuffer<OctahedralParams> params;

/// `extent * extent` unit directions in `xyz`, one per texel of a layer, row
/// major — `crcbl_shaders::probe_visibility::texel_direction` evaluated on the
/// host. `w` is that direction's cube face as a whole number, from
/// `crcbl_render::probe_capture::face_of`.
[[vk::binding(1, 0)]]
StructuredBuffer<float4> directions;

/// Every probe's world → face-clip matrices, six per probe in face order, for
/// the **whole** capture rather than for this chunk. One buffer for every
/// dispatch is what lets the chunks share an encoder: a host-visible buffer
/// rewritten between two recorded dispatches would hold only its last contents
/// by the time either of them ran.
[[vk::binding(2, 0)]]
StructuredBuffer<float4x4> faces;

/// The distance atlas `probe_capture.slang` filled: `R32Float`, read with
/// `Load` and no sampler, which is what an unfilterable format allows.
[[vk::binding(3, 0)]]
Texture2D<float> distances;

/// The moments, as flat floats: `layer_floats` a probe, `row_floats` a row, two
/// per texel. Copied into the `Rg32Float` array image afterwards rather than
/// written as a storage image, so the layout question is a buffer offset rather
/// than a per-backend storage-format capability.
[[vk::binding(4, 0)]]
RWStructuredBuffer<float> moments;

[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint texels = params.extent * params.extent;
    uint index = thread.x;
    if (index >= params.probes * texels)
    {
        return;
    }

    uint probe = index / texels;
    uint texel = index - probe * texels;
    uint row = texel / params.extent;
    uint column = texel - row * params.extent;

    float4 entry = directions[texel];
    float3 direction = entry.xyz;
    uint face = uint(entry.w);
    // Chunk-local for the atlas, whose tiles are this dispatch's alone, and
    // capture-global for the matrix, whose buffer spans every probe.
    uint tile = probe * 6u + face;
    uint slot = (params.probe_base + probe) * 6u + face;

    // `w = 0`: the header says why the probe's position is not needed here.
    float4 clip = mul(faces[slot], float4(direction, 0.0));
    float3 ndc = clip.xyz / clip.w;
    // `mesh.slang`'s shadow taps' mapping exactly — the `y` flip is the
    // framebuffer's, and getting it wrong reads the tile upside down.
    float2 uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);

    float side = float(params.face_texels);
    // Clamped rather than trusted: `uv` is inside the tile by construction for
    // the face the major axis picked, but an out-of-range `Load` is undefined
    // rather than clamped and the product above is a float.
    float2 inside = clamp(uv * side, float2(0.0, 0.0), float2(side - 1.0, side - 1.0));
    uint2 corner = uint2((tile % params.atlas_columns) * params.face_texels,
                         (tile / params.atlas_columns) * params.face_texels);
    int2 coordinate = int2(corner + uint2(inside));
    float reach = distances.Load(int3(coordinate, 0));

    uint at = (params.probe_base + probe) * params.layer_floats
            + row * params.row_floats
            + column * 2u;
    moments[at] = reach;
    moments[at + 1u] = reach * reach;
}
