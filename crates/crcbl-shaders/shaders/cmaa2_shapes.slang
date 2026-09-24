// crcbl-targets: spirv, wgsl, msl, dxil

// The antialiasing ladder's CMAA2 tier, second of its three passes: the
// shape classification with its bounded long-edge search, which adds every
// share it derives straight into the per-pixel fixed-point accumulation.
// `cmaa2_edges.slang`'s header describes the tier, what each pass hands the
// next, and which parts of the reference this transcribes.
//
// # The shapes, and where their coverage comes from
//
// A candidate is a pixel with an edge on its west or north boundary. Runs of
// those boundaries are the aliased staircase, and each run is classified by
// **how the boundary turns at its two ends** — which is the 2×2 pattern the
// reference calls a `Z` or a `U`:
//
// * **`Z`** — the two ends turn opposite ways. The boundary is a straight line
//   crossing the run diagonally, and reconstructing it is exact: at the run's
//   left end it sits half a pixel to one side of the aliased boundary and at
//   the right end half a pixel to the other, so the coverage it leaves over
//   run pixel `i` is the line's mean offset across that pixel.
// * **`U`** — the two ends turn the same way. No straight line has those
//   endpoints, so the reconstruction is two segments meeting in the middle: a
//   half-pixel offset at each end falling to zero between them. That is the
//   morphological family's own answer and it over-blurs a genuine notch, so it
//   is taken at `U_SHAPE_WEIGHT` rather than whole — the "conservative" the
//   tier is named for.
// * **Neither** — an end that does not turn at all, because the run leaves the
//   frame or reaches `MAX_LINE_LENGTH` before it terminates. Nothing is
//   emitted: an unclassified run is a run that keeps its unresolved colour.
//
// The coverage is derived here rather than looked up, which is the structural
// difference from the SMAA tier this replaces — that one carried two committed
// tables and this one carries none.
//
// # One invocation per pixel, and why that is not the same as per candidate
//
// This pass used to run once per entry of a **candidate list** the edge pass
// appended to, sized as a fraction of the frame, which dropped whatever arrived
// past its capacity. `cmaa2_edges.slang`'s header carries what that cost: the
// picture became a function of the device's scheduling on any frame dense
// enough to fill the list. So the list is gone and the dispatch covers the
// frame, with each invocation testing the same predicate the append tested — a
// non-zero `edges` word. The work per candidate is unchanged; a non-candidate
// costs one buffer read and an early return.
//
// # Fixed point, because a float sum in arrival order is not a function of its
// inputs
//
// A run's shares land on pixels other runs also land on, so a pixel's total is
// a sum over contributions that reach it in whatever order the device
// schedules. **Float addition is not associative**, so a float sum in arrival
// order would make a frame a function of the scheduler rather than of its
// inputs — which is the property this workspace's determinism rule spends its
// length refusing, and which no golden could hold. Integer addition is
// associative and commutative, so the accumulation is in fixed point at
// `BLEND_FIXED_POINT_SCALE` and the one conversion back to float happens per
// pixel in `cmaa2_apply.slang`, after every share has landed.
//
// That "after" is why the resolve is a separate dispatch: a pixel cannot read
// its own total until every workgroup that might add to it has finished, and
// the only ordering this engine has between workgroups is a pass boundary.
//
// # No `#include`
//
// `cmaa2_edges.slang` repeats `bloom_composite.slang`'s rule. The parameter
// block and the four resource declarations below are that file's, copied,
// because one bind-group layout serves both.

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

/// Everything the CMAA2 passes take from the CPU — `cmaa2_edges.slang` declares
/// the same block and its header says why there is one.
struct Cmaa2Params
{
    /// Width of the image being filtered, in texels.
    uint viewport_x;
    /// Its height.
    uint viewport_y;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`. These four are `cmaa2_edges.slang`'s, in
// its order, because one layout serves both files.

[[vk::binding(0, 0)]]
ConstantBuffer<Cmaa2Params> params D3D12_REGISTER(b0, space0);

/// The tonemapped frame, read for the colour a share carries part of.
[[vk::binding(1, 0)]]
Texture2D<float4> source D3D12_REGISTER(t1, space0);

/// `[pixel]`: the edge bits `cmaa2_edges.slang` wrote.
[[vk::binding(2, 0)]]
RWStructuredBuffer<uint> edges D3D12_REGISTER(u2, space0);

/// `[pixel * ACCUM_WORDS]`: the fixed-point accumulation, three colour words
/// and a weight word per pixel.
[[vk::binding(3, 0)]]
RWStructuredBuffer<Atomic<uint> > accum D3D12_REGISTER(u3, space0);

/// Words per pixel in `accum`, matching `crcbl_shaders::cmaa2::ACCUM_WORDS`.
static const uint ACCUM_WORDS = 4;

/// Bit 0 of an `edges` word: an edge on the pixel's west boundary.
static const uint EDGE_LEFT = 1;

/// Bit 1: an edge on its north boundary.
static const uint EDGE_TOP = 2;

/// The longest run this pass will classify, in pixels — matching
/// `crcbl_shaders::cmaa2::MAX_LINE_LENGTH`.
///
/// A bound rather than a quality knob: the search is a loop and an unbounded
/// one over a frame-sized buffer is a shader a driver may time out on. A run
/// longer than this is left unclassified rather than truncated and guessed at,
/// because a truncated run's ends are not its ends and the coverage derived
/// from them would be a shape the frame does not contain.
static const uint MAX_LINE_LENGTH = 64;

/// What a fixed-point weight of one is — matching
/// `crcbl_shaders::cmaa2::BLEND_FIXED_POINT_SCALE`.
///
/// Two to the twentieth. The precision argument is in two halves and both
/// matter. **Below**: one part in 1048576 is three orders of magnitude finer
/// than the `1/255` step of an eight-bit target, so the quantisation cannot
/// move a written texel. **Above**: the accumulation has no path to a wrap, and
/// `crcbl_shaders::cmaa2`'s headroom test is where that is arithmetic rather
/// than prose — it derives the largest total a pixel can be given from
/// `MAX_LINE_LENGTH`, `MAX_BLEND_WEIGHT` and the walk directions below, and
/// compares it against what a `uint` holds.
static const float BLEND_FIXED_POINT_SCALE = 1048576.0;

/// How much of the `U` reconstruction is applied — matching
/// `crcbl_shaders::cmaa2::U_SHAPE_WEIGHT`.
///
/// A `U` is the one shape whose reconstruction is a guess: two segments meeting
/// in the middle is only one of the surfaces that could have produced those
/// ends, and taking it whole softens a notch that may be geometry rather than
/// aliasing. **This transcription's number, not the reference's** — see
/// `cmaa2_edges.slang`'s header and `docs/backlog.md`.
static const float U_SHAPE_WEIGHT = 0.5;

/// The smallest share worth accumulating, as a fraction.
///
/// Half the `1/255` step of an eight-bit target: a contribution below it cannot
/// move the pixel it lands on, and every one of them is four atomic adds and a
/// texel fetch.
static const float MIN_BLEND_WEIGHT = 0.001953125;

/// The largest share one contribution may carry.
///
/// Half: a reconstructed line is at most half a pixel off the aliased boundary
/// it replaces, so a pixel is at most half its neighbour's.
static const float MAX_BLEND_WEIGHT = 0.5;

/// The edge bits of `(x, y)`, or none where that is outside the frame.
///
/// `x` and `y` are unsigned, so a step off the west or north side wraps to a
/// very large number and is caught by the same comparison as a step off the
/// east or south one.
uint edge_at(uint x, uint y)
{
    if (x >= params.viewport_x || y >= params.viewport_y)
    {
        return 0;
    }
    return edges[y * params.viewport_x + x];
}

/// Which way a horizontal run turns at the boundary column `column` of row
/// `row`: `1` where the vertical segment continuing it lies in the row above,
/// `-1` where it lies in this row, and `0` where there is neither or both.
///
/// A run of north boundaries along row `row` separates the pixels of row
/// `row - 1` from those of row `row`. Where it ends, the boundary has to keep
/// going vertically, and it can only do that as a west boundary in one of those
/// two rows.
int horizontal_turn(uint column, uint row)
{
    bool below = (edge_at(column, row) & EDGE_LEFT) != 0;
    bool above = row > 0 && (edge_at(column, row - 1) & EDGE_LEFT) != 0;
    if (above && !below)
    {
        return 1;
    }
    if (below && !above)
    {
        return -1;
    }
    return 0;
}

/// The same for a vertical run, at the boundary row `row` of column `column`:
/// `1` where the horizontal segment continuing it lies in the column to the
/// west, `-1` where it lies in this column.
int vertical_turn(uint column, uint row)
{
    bool right = (edge_at(column, row) & EDGE_TOP) != 0;
    bool left = column > 0 && (edge_at(column - 1, row) & EDGE_TOP) != 0;
    if (left && !right)
    {
        return 1;
    }
    if (right && !left)
    {
        return -1;
    }
    return 0;
}

/// Where the reconstructed boundary sits, relative to the aliased one, a
/// fraction `t` of the way along a run.
///
/// In pixels and **signed**: negative is toward the pixel on the `-offset` side
/// of the boundary, positive toward the one on the other side. A half-pixel
/// offset at an end is the most a morphological reconstruction can claim, which
/// is where the halves come from.
///
/// A `Z` is a straight line between the two ends. A `U` cannot be one — no
/// straight line has both ends on the same side — so it is two segments meeting
/// at zero in the middle, taken at [`U_SHAPE_WEIGHT`].
float boundary_height(float t, int start, int end, bool u_shape)
{
    if (u_shape)
    {
        return -0.5 * float(start) * (2.0 * abs(t - 0.5)) * U_SHAPE_WEIGHT;
    }
    return -0.5 * float(start) * (1.0 - t) - 0.5 * float(end) * t;
}

/// Adds `target`'s share of the colour at `from` into the accumulation.
///
/// **Four integer atomic adds and nothing read back**, which is what makes the
/// total a pixel ends the pass with independent of the order the invocations
/// producing it ran in. Nothing is queued, nothing is dropped, and there is no
/// capacity to exceed — `cmaa2_edges.slang`'s header carries what the append
/// list this replaces cost.
///
/// A share below [`MIN_BLEND_WEIGHT`] is skipped: it cannot move the pixel it
/// lands on, and it is still four atomics and a fetch.
///
/// The weight is quantised **once** and the colour is scaled by the quantised
/// value, so the three colour words and the weight word stay consistent with
/// each other — a colour scaled by the unrounded share and divided by the
/// rounded one in `cmaa2_apply.slang` would not be the colour that was read.
///
/// The two index guards are not reachable from the walks below, which stay
/// inside the frame by construction — a run's pixels all carry the boundary bit
/// that says the pixel across it exists — and they are here because these are
/// unchecked buffer indices: the cost is a comparison and what it buys is that
/// a future walk cannot scribble outside the accumulation.
void accumulate(uint target, uint from, float share)
{
    if (share < MIN_BLEND_WEIGHT)
    {
        return;
    }
    uint pixels = params.viewport_x * params.viewport_y;
    if (target >= pixels || from >= pixels)
    {
        return;
    }

    uint width = params.viewport_x;
    int2 texel = int2(int(from % width), int(from / width));
    // Saturated, so the products below cannot exceed the weight and the
    // headroom argument holds for any frame — including one an earlier pass
    // left above one.
    float3 color = saturate(source.Load(int3(texel, 0)).rgb);
    uint weight = uint(min(share, MAX_BLEND_WEIGHT) * BLEND_FIXED_POINT_SCALE);
    float scaled = float(weight);

    accum[target * ACCUM_WORDS + 0].add(uint(color.r * scaled));
    accum[target * ACCUM_WORDS + 1].add(uint(color.g * scaled));
    accum[target * ACCUM_WORDS + 2].add(uint(color.b * scaled));
    accum[target * ACCUM_WORDS + 3].add(weight);
}

/// Accumulates the shares one classified run leaves.
///
/// `first` is the pixel index of the run's first element, `stride` the index
/// step between consecutive elements — one along a row, a row's width down a
/// column — and `offset` the index distance to the pixel on the *other* side of
/// the run's boundary, which is a row's width for a north boundary and one for
/// a west boundary.
///
/// `start` and `end` are [`horizontal_turn`]'s or [`vertical_turn`]'s answers at
/// the two ends. A run with a zero at either end is not a shape and leaves
/// nothing.
///
/// # The coverage is an integral over the pixel, not the height at its middle
///
/// Both sides of the boundary can be wrong inside **one** pixel, and taking the
/// mean signed height would cancel them. The clearest case is the one this tier
/// exists for: a 45° staircase, where every run is a single pixel long and the
/// reconstructed line runs corner to corner across it. Its mean height is
/// exactly zero — and an eighth of the pixel above the line belongs below it and
/// an eighth below belongs above, which is a mix a filter must make and a mean
/// says is unnecessary. So the two areas are integrated separately, and a pixel
/// the line crosses contributes **two** shares.
void blend_line(uint first, uint stride, uint len, uint offset, int start, int end)
{
    if (start == 0 || end == 0)
    {
        return;
    }
    float inv_len = 1.0 / float(len);
    bool u_shape = start == end;
    for (uint i = 0; i < len; ++i)
    {
        // The reconstructed boundary at this element's two edges. It is linear
        // between them — a `U`'s only bend is at the middle of the run, which
        // falls on an element edge whenever the run has an even length and is
        // inside one element when it does not, where the error is a fraction of
        // one pixel's coverage.
        float near = boundary_height(float(i) * inv_len, start, end, u_shape);
        float far = boundary_height(float(i + 1) * inv_len, start, end, u_shape);

        // The area of this element on each side of the reconstruction: a
        // trapezoid where the two heights share a sign, and two triangles where
        // the line crosses inside the element.
        float below;
        float above;
        if (near <= 0.0 && far <= 0.0)
        {
            below = -0.5 * (near + far);
            above = 0.0;
        }
        else if (near >= 0.0 && far >= 0.0)
        {
            below = 0.0;
            above = 0.5 * (near + far);
        }
        else
        {
            float crossing = near / (near - far);
            below = 0.5 * abs(min(near, far)) * (near < 0.0 ? crossing : 1.0 - crossing);
            above = 0.5 * max(near, far) * (near > 0.0 ? crossing : 1.0 - crossing);
        }

        uint pixel = first + i * stride;
        // A negative height puts the reconstructed boundary on the `-offset`
        // side of the aliased one, so `below` is area the pixel across the
        // boundary holds that belongs to this side: that pixel takes this one's
        // colour by that share. `above` is the mirror — area this pixel holds
        // that belongs across — and this pixel takes the colour from across.
        // Handing each share to the other pixel blends the one that was right
        // and leaves the one that was wrong, which is one darkened corner per
        // step and a staircase that stays a staircase.
        accumulate(pixel - offset, pixel, below);
        accumulate(pixel, pixel - offset, above);
    }
}

/// One invocation per pixel: classify the runs that **start** at this pixel and
/// accumulate their shares.
///
/// A pixel with no edge word returns immediately — that is the predicate the
/// retired candidate list was filled on, tested in place. Of the pixels that do
/// carry one, only a run's first does the walk, so a run of length `n` is
/// walked once rather than `n` times. A pixel may start a horizontal run and a
/// vertical one, and both are processed.
[shader("compute")]
[numthreads(64, 1, 1)]
void shapesMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    uint width = params.viewport_x;
    uint pixels = width * params.viewport_y;
    if (index >= pixels)
    {
        return;
    }

    uint own = edges[index];
    if (own == 0)
    {
        return;
    }

    uint x = index % width;
    uint y = index / width;

    // A horizontal run starts here when this pixel carries a north boundary and
    // the pixel to its west does not.
    if ((own & EDGE_TOP) != 0 && (edge_at(x - 1, y) & EDGE_TOP) == 0)
    {
        uint len = 1;
        while (len <= MAX_LINE_LENGTH && (edge_at(x + len, y) & EDGE_TOP) != 0)
        {
            len += 1;
        }
        if (len <= MAX_LINE_LENGTH)
        {
            blend_line(index, 1, len, width, horizontal_turn(x, y), horizontal_turn(x + len, y));
        }
    }

    // A vertical run starts here when this pixel carries a west boundary and
    // the pixel to its north does not.
    if ((own & EDGE_LEFT) != 0 && (edge_at(x, y - 1) & EDGE_LEFT) == 0)
    {
        uint len = 1;
        while (len <= MAX_LINE_LENGTH && (edge_at(x, y + len) & EDGE_LEFT) != 0)
        {
            len += 1;
        }
        if (len <= MAX_LINE_LENGTH)
        {
            blend_line(index, width, len, 1, vertical_turn(x, y), vertical_turn(x, y + len));
        }
    }
}
