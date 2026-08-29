//! SMAA's two lookup tables — the area table its blend-weight pass reads and
//! the search table its line search ends on — as committed bytes, with the
//! generator beside them.
//!
//! `docs/plan/49-antialiasing.md`'s second rung is SMAA 1x: an edge detection,
//! a blend-weight pass and a neighbourhood blend. The middle pass looks the
//! edge pattern it found up in two precomputed tables, and the plan wants
//! those to arrive as **committed bytes** with a generator and a `--check`
//! behind them rather than be derived at start-up — a table four rasterisers
//! computed independently would otherwise sit underneath every golden. This
//! module is a transcription of the reference generators, `Scripts/AreaTex.py`
//! and `Scripts/SearchTex.py` in the SMAA repository (Jimenez et al.,
//! `iryoku/smaa`), kept operation for operation so the bytes here are the
//! bytes `SMAA.hlsl` was written against. It was verified byte-for-byte
//! against those scripts' own output on 2026-08-30: the search table whole,
//! and the area table at every subsample offset the reference bakes, of which
//! the committed slab is the first.
//!
//! # What is committed, and what is not
//!
//! The reference area texture is 160×560: seven 80-row slabs of orthogonal
//! patterns down the left half, one per subsample offset, beside five slabs of
//! diagonal ones down the right. Every slab but the first exists for SMAA S2x
//! and T2x, which need a multisampled or a temporally jittered frame — and
//! neither has a rung here (`docs/plan/49-antialiasing.md` keeps TAA post-MVP
//! and MSAA unscheduled). SMAA 1x reads the first slab only, so that is what
//! `tables/smaa_area.bin` holds: [`AREA_WIDTH`]×[`AREA_HEIGHT`] texels of two
//! bytes, the orthogonal block on the left and the diagonal block on the
//! right, exactly the layout the reference gives its first 80 rows. A shader
//! reading it addresses it with these constants rather than the reference's
//! `SMAA_AREATEX_PIXEL_SIZE`, and `SMAA_AREATEX_SUBTEX_SIZE` has nothing to
//! select. [`bake_area_slab`] still takes the offsets, because the reference
//! does and a transcription that dropped an argument could not be held to it.
//!
//! The search table, `tables/smaa_search.bin`, is the reference's whole:
//! [`SEARCH_WIDTH`]×[`SEARCH_HEIGHT`] bytes.
//!
//! # Determinism
//!
//! Both generators are `f64` arithmetic the way the scripts are Python floats —
//! the same IEEE operations in the same order — and neither takes a `sin` or a
//! `cos`: an orthogonal area is a trapezoid or two triangles, the smoothing of
//! a U shape is a square root (correctly rounded on every platform), and a
//! diagonal area counts samples on one side of a line. So `cook-smaa --check`
//! compares bytes exactly, where `cook-dfg` has to allow a tolerance.
//!
//! Regenerate or verify with the tool that owns them:
//!
//! ```text
//! cargo run -p crcbl-shaders --example cook-smaa            # regenerate
//! cargo run -p crcbl-shaders --example cook-smaa -- --check # verify only
//! ```

/// Distances the orthogonal patterns are tabulated for, along each axis: the
/// reference's `SIZE_ORTHO`, and `SMAA_AREATEX_MAX_DISTANCE` in the shader.
///
/// A texel at distance index `i` holds the area at distance `i²` — the
/// reference compresses the axis quadratically so sixteen texels reach a
/// search of 225 pixels.
pub const AREA_MAX_DISTANCE: usize = 16;

/// Distances the diagonal patterns are tabulated for: the reference's
/// `SIZE_DIAG`, and `SMAA_AREATEX_MAX_DISTANCE_DIAG` in the shader. Linear,
/// not compressed.
pub const AREA_MAX_DISTANCE_DIAG: usize = 20;

/// The orthogonal block's side: five edge codes along each axis, one
/// [`AREA_MAX_DISTANCE`]-square pattern tile per code pair.
const ORTHO_BLOCK: usize = 5 * AREA_MAX_DISTANCE;

/// The diagonal block's side: four edge codes along each axis.
const DIAG_BLOCK: usize = 4 * AREA_MAX_DISTANCE_DIAG;

/// Texels across the area table: the orthogonal block, then the diagonal one.
pub const AREA_WIDTH: usize = ORTHO_BLOCK + DIAG_BLOCK;

/// Texels down the area table: one slab, which both blocks fill exactly.
pub const AREA_HEIGHT: usize = ORTHO_BLOCK;
const _: () = assert!(
    DIAG_BLOCK == ORTHO_BLOCK,
    "the two blocks share one slab height"
);

/// Bytes one area texel occupies: the area below the line, then the area
/// above it, each as `⌊255·a⌋` — an `Rg8Unorm` texel.
pub const AREA_TEXEL_BYTES: usize = 2;

/// The committed area table's exact length.
///
/// The artifact is typed `&[u8; AREA_BYTES]` where it is included, so a table
/// of the wrong size fails to compile rather than being caught by a test.
pub const AREA_BYTES: usize = AREA_WIDTH * AREA_HEIGHT * AREA_TEXEL_BYTES;

/// Texels across the search table, after the reference's crop to a power of
/// two: the left-search half, then the right-search half, 32 texels each.
pub const SEARCH_WIDTH: usize = 64;

/// Texels down the search table: the sixteen rows of the reference's 33 that
/// its crop keeps, flipped so the last row comes first.
pub const SEARCH_HEIGHT: usize = 16;

/// The committed search table's exact length: one byte per texel, an
/// `R8Unorm` image holding `0`, `127` or `254` — the step to take, times 127.
pub const SEARCH_BYTES: usize = SEARCH_WIDTH * SEARCH_HEIGHT;

/// The step a search texel encodes is stored times this, "to maximize dynamic
/// range to help compression" in the reference's words; the shader divides it
/// back out.
pub const SEARCH_STEP_SCALE: u8 = 127;

/// Samples along each axis of a diagonal area's brute-force count: the
/// reference's `SAMPLES_DIAG`.
const DIAG_SAMPLES: usize = 30;

/// The distance past which a U shape stops being smoothed toward its
/// square-root blend: the reference's `SMOOTH_MAX_DISTANCE`.
const SMOOTH_MAX_DISTANCE: f64 = 32.0;

/// Where each orthogonal pattern's tile sits, in tiles, as `(across, down)`:
/// the reference's `edgesortho`, indexed by the four-bit edge code.
const EDGES_ORTHO: [(usize, usize); 16] = [
    (0, 0),
    (3, 0),
    (0, 3),
    (3, 3),
    (1, 0),
    (4, 0),
    (1, 3),
    (4, 3),
    (0, 1),
    (3, 1),
    (0, 4),
    (3, 4),
    (1, 1),
    (4, 1),
    (1, 4),
    (4, 4),
];

/// Where each diagonal pattern's tile sits: the reference's `edgesdiag`. The
/// two entries also say which end of the pattern's line a subsample offset
/// moves — a non-zero code moves that end.
const EDGES_DIAG: [(usize, usize); 16] = [
    (0, 0),
    (1, 0),
    (0, 2),
    (1, 2),
    (2, 0),
    (3, 0),
    (2, 2),
    (3, 2),
    (0, 1),
    (1, 1),
    (0, 3),
    (1, 3),
    (2, 1),
    (3, 1),
    (2, 3),
    (3, 3),
];

/// The committed area table, `tables/smaa_area.bin`.
///
/// In the binary, exactly as the compiled shaders are, so there is no file for
/// a deployment to lose.
const AREA_TABLE: &[u8; AREA_BYTES] = include_bytes!("../tables/smaa_area.bin");

/// The committed search table, `tables/smaa_search.bin`.
const SEARCH_TABLE: &[u8; SEARCH_BYTES] = include_bytes!("../tables/smaa_search.bin");

/// The committed area table's bytes, for a caller uploading it to a device.
///
/// Row-major, [`AREA_WIDTH`] texels of [`AREA_TEXEL_BYTES`] per row, which is
/// the order a 2D image upload expects and the order [`area_entry`] indexes in.
#[must_use]
pub const fn area_bytes() -> &'static [u8; AREA_BYTES] {
    AREA_TABLE
}

/// The committed search table's bytes, row-major, one per texel.
#[must_use]
pub const fn search_bytes() -> &'static [u8; SEARCH_BYTES] {
    SEARCH_TABLE
}

/// The committed area texel at `(x, y)`, as `[below, above]`.
///
/// # Panics
///
/// If `x` is at or past [`AREA_WIDTH`] or `y` at or past [`AREA_HEIGHT`].
#[must_use]
pub fn area_entry(x: usize, y: usize) -> [u8; AREA_TEXEL_BYTES] {
    assert!(
        x < AREA_WIDTH && y < AREA_HEIGHT,
        "({x}, {y}) is outside a {AREA_WIDTH}x{AREA_HEIGHT} table"
    );
    let at = (y * AREA_WIDTH + x) * AREA_TEXEL_BYTES;
    [AREA_TABLE[at], AREA_TABLE[at + 1]]
}

/// The committed search texel at `(x, y)`.
///
/// # Panics
///
/// If `x` is at or past [`SEARCH_WIDTH`] or `y` at or past [`SEARCH_HEIGHT`].
#[must_use]
pub fn search_entry(x: usize, y: usize) -> u8 {
    assert!(
        x < SEARCH_WIDTH && y < SEARCH_HEIGHT,
        "({x}, {y}) is outside a {SEARCH_WIDTH}x{SEARCH_HEIGHT} table"
    );
    SEARCH_TABLE[y * SEARCH_WIDTH + x]
}

/// `a + (b − a)·p`, as the reference spells it — the order matters to a
/// byte-exact transcription.
fn lerp(a: f64, b: f64, p: f64) -> f64 {
    a + (b - a) * p
}

/// `⌊255·a⌋` as the reference's `bytes()` takes it: truncated, never rounded.
fn quantise(a: f64) -> u8 {
    let scaled = 255.0 * a;
    assert!(
        (0.0..=255.0).contains(&scaled),
        "an area of {a} is outside the unit square"
    );
    // Truncation toward zero is what `int()` does to a positive float, and
    // the range was asserted just above.
    scaled as u8
}

/// The area under the line `p1 → p2` inside the pixel column `x .. x + 1`, as
/// `[below, above]` shares of that pixel — the reference's inner `area()` for
/// orthogonal patterns.
///
/// A column the line crosses without changing sign is a trapezoid; one where
/// it does change sign splits into two triangles, and the larger decides which
/// side each goes to. A column outside the line's span is nothing.
fn ortho_area(p1: [f64; 2], p2: [f64; 2], x: usize) -> [f64; 2] {
    let d = [p2[0] - p1[0], p2[1] - p1[1]];
    let x1 = x as f64;
    let x2 = x as f64 + 1.0;
    let y1 = p1[1] + d[1] * (x1 - p1[0]) / d[0];
    let y2 = p1[1] + d[1] * (x2 - p1[0]) / d[0];

    let inside = (x1 >= p1[0] && x1 < p2[0]) || (x2 > p1[0] && x2 <= p2[0]);
    if !inside {
        return [0.0, 0.0];
    }
    // `copysign(1.0, y1) == copysign(1.0, y2)` in the reference: the sign bit,
    // so a negative zero counts as negative there and here.
    let is_trapezoid =
        y1.is_sign_negative() == y2.is_sign_negative() || y1.abs() < 1e-4 || y2.abs() < 1e-4;
    if is_trapezoid {
        let a = (y1 + y2) / 2.0;
        if a < 0.0 {
            [a.abs(), 0.0]
        } else {
            [0.0, a.abs()]
        }
    } else {
        // Where the line crosses zero, and `fract` is `modf`'s fractional part:
        // the same sign as the whole.
        let x = -p1[1] * d[0] / d[1] + p1[0];
        let a1 = if x > p1[0] { y1 * x.fract() / 2.0 } else { 0.0 };
        let a2 = if x < p2[0] {
            y2 * (1.0 - x.fract()) / 2.0
        } else {
            0.0
        };
        let a = if a1.abs() > a2.abs() { a1 } else { -a2 };
        if a < 0.0 {
            [a1.abs(), a2.abs()]
        } else {
            [a2.abs(), a1.abs()]
        }
    }
}

/// The reference's `smootharea`: blends a short U shape's two halves toward
/// their square roots, so a one-pixel U does not vanish.
fn smooth_area(d: f64, a1: [f64; 2], a2: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    let b1 = [(a1[0] * 2.0).sqrt() * 0.5, (a1[1] * 2.0).sqrt() * 0.5];
    let b2 = [(a2[0] * 2.0).sqrt() * 0.5, (a2[1] * 2.0).sqrt() * 0.5];
    let p = (d / SMOOTH_MAX_DISTANCE).clamp(0.0, 1.0);
    (
        [lerp(b1[0], a1[0], p), lerp(b1[1], a1[1], p)],
        [lerp(b2[0], a2[0], p), lerp(b2[1], a2[1], p)],
    )
}

/// The reference's `areaortho`: the `[below, above]` area for an orthogonal
/// edge `pattern` (the four-bit code the shader builds from the crossing edges
/// it found) at `left` and `right` pixels from the pixel in question, with the
/// line's ends biased by a subsample `offset` — zero for SMAA 1x.
///
/// The comments name the shapes as the reference draws them; its own reasons
/// for each arm are its to give, and this transcribes the arithmetic.
fn area_ortho(pattern: usize, left: usize, right: usize, offset: f64) -> [f64; 2] {
    let d = (left + right + 1) as f64;
    let o1 = 0.5 + offset;
    let o2 = 0.5 + offset - 1.0;
    let half = [d / 2.0, 0.0];
    let sum = |a: [f64; 2], b: [f64; 2]| [a[0] + b[0], a[1] + b[1]];
    let mean = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    match pattern {
        // `------`, and `+------`, `------+`, `+------+`: nothing to blend.
        0 | 5 | 10 | 15 => [0.0, 0.0],
        // `.------`: an L, offset on its crossing side only, and only on the
        // shorter arm, so it converges with the unfiltered pattern 0.
        1 => {
            if left <= right {
                ortho_area([0.0, o2], half, left)
            } else {
                [0.0, 0.0]
            }
        }
        // `------.`
        2 => {
            if left >= right {
                ortho_area(half, [d, o2], left)
            } else {
                [0.0, 0.0]
            }
        }
        // `.------.`: a U, smoothed.
        3 => {
            let a1 = ortho_area([0.0, o2], half, left);
            let a2 = ortho_area(half, [d, o2], left);
            let (a1, a2) = smooth_area(d, a1, a2);
            sum(a1, a2)
        }
        // `` `------ ``
        4 => {
            if left <= right {
                ortho_area([0.0, o1], half, left)
            } else {
                [0.0, 0.0]
            }
        }
        // `` `------. ``: a Z. With an offset, the full revectorisation is
        // blended with the two partially offset Ls so the pattern the centre
        // sees meets the one the sides see.
        6 => {
            if offset.abs() > 0.0 {
                let a1 = ortho_area([0.0, o1], [d, o2], left);
                let a2 = sum(
                    ortho_area([0.0, o1], half, left),
                    ortho_area(half, [d, o2], left),
                );
                mean(a1, a2)
            } else {
                ortho_area([0.0, o1], [d, o2], left)
            }
        }
        // `+------.`
        7 => ortho_area([0.0, o1], [d, o2], left),
        // `` ------´ ``
        8 => {
            if left >= right {
                ortho_area(half, [d, o1], left)
            } else {
                [0.0, 0.0]
            }
        }
        // `` .------´ ``: the other Z.
        9 => {
            if offset.abs() > 0.0 {
                let a1 = ortho_area([0.0, o2], [d, o1], left);
                let a2 = sum(
                    ortho_area([0.0, o2], half, left),
                    ortho_area(half, [d, o1], left),
                );
                mean(a1, a2)
            } else {
                ortho_area([0.0, o2], [d, o1], left)
            }
        }
        // `.------+`
        11 => ortho_area([0.0, o2], [d, o1], left),
        // `` `------´ ``: the other U.
        12 => {
            let a1 = ortho_area([0.0, o1], half, left);
            let a2 = ortho_area(half, [d, o1], left);
            let (a1, a2) = smooth_area(d, a1, a2);
            sum(a1, a2)
        }
        // `` +------´ ``
        13 => ortho_area([0.0, o2], [d, o1], left),
        // `` `------+ ``
        14 => ortho_area([0.0, o1], [d, o2], left),
        _ => unreachable!("an edge pattern is four bits"),
    }
}

/// The share of the unit pixel at `p` lying on the positive side of the line
/// `p1 → p2`, counted over a [`DIAG_SAMPLES`]-square lattice — the
/// reference's `area1`, "quick and dirty" by its own account and kept so.
fn diag_area1(p1: [f64; 2], p2: [f64; 2], p: [f64; 2]) -> f64 {
    let step = (DIAG_SAMPLES - 1) as f64;
    let mut a = 0.0;
    for x in 0..DIAG_SAMPLES {
        for y in 0..DIAG_SAMPLES {
            let o = [x as f64 / step, y as f64 / step];
            let q = [p[0] + o[0], p[1] + o[1]];
            let inside = if p1 == p2 {
                true
            } else {
                let xm = (p1[0] + p2[0]) / 2.0;
                let ym = (p1[1] + p2[1]) / 2.0;
                let a = p2[1] - p1[1];
                let b = p1[0] - p2[0];
                let c = a * (q[0] - xm) + b * (q[1] - ym);
                c > 0.0
            };
            if inside {
                a += 1.0;
            }
        }
    }
    a / (DIAG_SAMPLES * DIAG_SAMPLES) as f64
}

/// The reference's diagonal `area`: the line `p1 → p2` with whichever ends
/// `pattern`'s code moves biased by `offset`, read for the pixel and the one
/// diagonally past it, as `[1 − below, above]`.
fn diag_area(
    pattern: usize,
    p1: [f64; 2],
    p2: [f64; 2],
    left: usize,
    offset: [f64; 2],
) -> [f64; 2] {
    let (e1, e2) = EDGES_DIAG[pattern];
    let shift = |p: [f64; 2], e: usize| {
        if e > 0 {
            [p[0] + offset[0], p[1] + offset[1]]
        } else {
            p
        }
    };
    let (p1, p2) = (shift(p1, e1), shift(p2, e2));
    let left = left as f64;
    let a1 = diag_area1(p1, p2, [1.0 + left, left]);
    let a2 = diag_area1(p1, p2, [1.0 + left, 1.0 + left]);
    [1.0 - a1, a2]
}

/// The reference's `areadiag`: the area for a diagonal edge `pattern` at
/// `left` and `right` pixels along it, offset by a subsample `offset` — zero
/// for SMAA 1x.
///
/// Where a pattern's end is not known — the reference's "black magic" — the
/// two possible endings are baked and averaged.
fn area_diag(pattern: usize, left: usize, right: usize, offset: [f64; 2]) -> [f64; 2] {
    let d = (left + right + 1) as f64;
    let area = |p1: [f64; 2], p2: [f64; 2]| diag_area(pattern, p1, p2, left, offset);
    let mean = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    // The line's two candidate starts and ends, in the reference's spelling.
    let (s00, s10, s11) = ([0.0, 0.0], [1.0, 0.0], [1.0, 1.0]);
    let (e00, e10, e11) = ([d, d], [1.0 + d, d], [1.0 + d, 1.0 + d]);
    match pattern {
        0 => mean(area(s11, e11), area(s10, e10)),
        1 => mean(area(s10, e00), area(s10, e10)),
        2 => mean(area(s00, e10), area(s10, e10)),
        3 => area(s10, e10),
        4 => mean(area(s11, e00), area(s11, e10)),
        5 => mean(area(s11, e00), area(s10, e10)),
        6 => area(s11, e10),
        7 => mean(area(s11, e10), area(s10, e10)),
        8 => mean(area(s00, e11), area(s10, e11)),
        9 => area(s10, e11),
        10 => mean(area(s00, e11), area(s10, e10)),
        11 => mean(area(s10, e11), area(s10, e10)),
        12 => area(s11, e11),
        13 => mean(area(s11, e11), area(s10, e11)),
        14 => mean(area(s11, e11), area(s11, e10)),
        15 => mean(area(s11, e11), area(s10, e10)),
        _ => unreachable!("an edge pattern is four bits"),
    }
}

/// One slab of the reference's area texture — its rows `80·k .. 80·k + 80`
/// for the orthogonal offset `SUBSAMPLE_OFFSETS_ORTHO[k]` and the diagonal one
/// `SUBSAMPLE_OFFSETS_DIAG[k]` — as [`AREA_BYTES`] row-major `Rg8` bytes.
///
/// [`bake_area`] is this at the zero offsets, which is the slab SMAA 1x
/// reads and the one committed; the offsets stay so the transcription can be
/// held to the whole reference texture, as the module header records it was.
#[must_use]
pub fn bake_area_slab(ortho_offset: f64, diag_offset: [f64; 2]) -> Vec<u8> {
    let mut bytes = vec![0u8; AREA_BYTES];
    let mut put = |x: usize, y: usize, area: [f64; 2]| {
        let at = (y * AREA_WIDTH + x) * AREA_TEXEL_BYTES;
        bytes[at] = quantise(area[0]);
        bytes[at + 1] = quantise(area[1]);
    };
    for (pattern, &(across, down)) in EDGES_ORTHO.iter().enumerate() {
        for left in 0..AREA_MAX_DISTANCE {
            for right in 0..AREA_MAX_DISTANCE {
                // The quadratic compression: texel `i` holds distance `i²`.
                let area = area_ortho(pattern, left * left, right * right, ortho_offset);
                put(
                    across * AREA_MAX_DISTANCE + left,
                    down * AREA_MAX_DISTANCE + right,
                    area,
                );
            }
        }
    }
    for (pattern, &(across, down)) in EDGES_DIAG.iter().enumerate() {
        for left in 0..AREA_MAX_DISTANCE_DIAG {
            for right in 0..AREA_MAX_DISTANCE_DIAG {
                let area = area_diag(pattern, left, right, diag_offset);
                put(
                    ORTHO_BLOCK + across * AREA_MAX_DISTANCE_DIAG + left,
                    down * AREA_MAX_DISTANCE_DIAG + right,
                    area,
                );
            }
        }
    }
    bytes
}

/// The area table SMAA 1x reads: [`bake_area_slab`] at the zero offsets, as
/// the bytes `tables/smaa_area.bin` holds.
#[must_use]
pub fn bake_area() -> Vec<u8> {
    bake_area_slab(0.0, [0.0, 0.0])
}

/// The reference's `bilinear`: what one fetch at `(−0.25, −0.125)` from the
/// pixel reads out of four binary edge values, which the search table is
/// indexed by so a single bilinear fetch of the edges texture becomes a lookup.
fn bilinear(e: [f64; 4]) -> f64 {
    let a = lerp(e[0], e[1], 1.0 - 0.25);
    let b = lerp(e[2], e[3], 1.0 - 0.25);
    lerp(a, b, 1.0 - 0.125)
}

/// The four edges a bilinear fetch value came from — the reverse of
/// [`bilinear`] — or `None` where no edge combination reads that value.
fn edges_for(fetch: f64) -> Option<[u8; 4]> {
    (0..16u8)
        .map(|code| [code >> 3 & 1, code >> 2 & 1, code >> 1 & 1, code & 1])
        .find(|&e| bilinear(e.map(f64::from)) == fetch)
}

/// The reference's `deltaLeft`: how many pixels a leftward search may still
/// advance, given the edges at its left and above it.
fn delta_left(left: [u8; 4], top: [u8; 4]) -> u8 {
    let mut d = 0;
    if top[3] == 1 {
        d += 1;
    }
    if d == 1 && top[2] == 1 && left[1] != 1 && left[3] != 1 {
        d += 1;
    }
    d
}

/// The reference's `deltaRight`.
fn delta_right(left: [u8; 4], top: [u8; 4]) -> u8 {
    let mut d = 0;
    if top[3] == 1 && left[1] != 1 && left[3] != 1 {
        d += 1;
    }
    if d == 1 && top[2] == 1 && left[0] != 1 && left[2] != 1 {
        d += 1;
    }
    d
}

/// The search table as the bytes `tables/smaa_search.bin` holds — the
/// reference's 66×33 image cropped to its rows 17 to 33 and columns 0 to 64,
/// then flipped top to bottom, which is what its `SearchTex.py` does before
/// writing.
#[must_use]
pub fn bake_search() -> Vec<u8> {
    const SIDE: usize = 33;
    // The uncropped image: left searches in the left half, right searches in
    // the right, and zero wherever a texel pair is not a fetch of real edges.
    let mut full = vec![0u8; 2 * SIDE * SIDE];
    for x in 0..SIDE {
        for y in 0..SIDE {
            let (Some(left), Some(top)) =
                (edges_for(0.03125 * x as f64), edges_for(0.03125 * y as f64))
            else {
                continue;
            };
            full[y * 2 * SIDE + x] = SEARCH_STEP_SCALE * delta_left(left, top);
            full[y * 2 * SIDE + SIDE + x] = SEARCH_STEP_SCALE * delta_right(left, top);
        }
    }
    let mut bytes = vec![0u8; SEARCH_BYTES];
    for row in 0..SEARCH_HEIGHT {
        let source = SIDE - 1 - row;
        bytes[row * SEARCH_WIDTH..(row + 1) * SEARCH_WIDTH]
            .copy_from_slice(&full[source * 2 * SIDE..source * 2 * SIDE + SEARCH_WIDTH]);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_tables_are_what_the_generators_produce() {
        assert!(
            bake_area().as_slice() == area_bytes().as_slice(),
            "tables/smaa_area.bin is not what bake_area produces — regenerate with cook-smaa"
        );
        assert!(
            bake_search().as_slice() == search_bytes().as_slice(),
            "tables/smaa_search.bin is not what bake_search produces — regenerate with cook-smaa"
        );
    }

    /// The reference's `SearchTex.py`, run on 2026-08-30 with its Pillow
    /// output captured as raw bytes, drawn a character per texel: `.` for a
    /// step of zero, `1` and `2` for the steps. Rows 0 and 7 are the two
    /// distinct non-empty shapes; every other non-empty row repeats one.
    const REFERENCE_SEARCH_ROW_0: &str =
        "22.11..22.11.........11.11..11.1121.....11......................";
    const REFERENCE_SEARCH_ROW_7: &str =
        "11.11..11.11.........11.11..11.1111.....11......................";

    fn search_row(y: usize) -> String {
        (0..SEARCH_WIDTH)
            .map(|x| match search_entry(x, y) {
                0 => '.',
                v if v == SEARCH_STEP_SCALE => '1',
                v if v == 2 * SEARCH_STEP_SCALE => '2',
                other => panic!("({x}, {y}) holds {other}, which is not a step"),
            })
            .collect()
    }

    #[test]
    fn the_search_table_is_the_reference_s() {
        assert_eq!(search_row(0), REFERENCE_SEARCH_ROW_0);
        assert_eq!(search_row(7), REFERENCE_SEARCH_ROW_7);
        let empty = ".".repeat(SEARCH_WIDTH);
        let expected = |y: usize| match y {
            0 | 1 | 3 | 4 => REFERENCE_SEARCH_ROW_0,
            7 | 8 | 10 | 11 => REFERENCE_SEARCH_ROW_7,
            _ => empty.as_str(),
        };
        for y in 0..SEARCH_HEIGHT {
            assert_eq!(search_row(y), expected(y), "row {y}");
        }
    }

    #[test]
    fn a_fetch_of_no_edges_and_of_all_four_are_the_ends_of_the_axis() {
        assert_eq!(edges_for(0.0), Some([0, 0, 0, 0]));
        assert_eq!(edges_for(1.0), Some([1, 1, 1, 1]));
        // 0.5 is reachable by no combination: the weights are 0.75 and 0.875,
        // so every fetch is a sum of 0.21875, 0.65625, 0.03125 and 0.09375.
        assert_eq!(edges_for(0.5), None);
    }

    #[test]
    fn a_search_stops_at_a_crossing_edge_and_runs_on_without_one() {
        // Edges below and continuing, nothing crossing: two more pixels.
        assert_eq!(delta_left([0, 0, 0, 0], [0, 0, 1, 1]), 2);
        assert_eq!(delta_right([0, 0, 0, 0], [0, 0, 1, 1]), 2);
        // A crossing edge on the left pixel's right side stops a left search
        // after one and a right search before it starts.
        assert_eq!(delta_left([0, 1, 0, 0], [0, 0, 1, 1]), 1);
        assert_eq!(delta_right([0, 1, 0, 0], [0, 0, 1, 1]), 0);
        // No edge under the pixel: nowhere to go.
        assert_eq!(delta_left([0, 0, 0, 0], [0, 0, 1, 0]), 0);
    }

    /// Where an orthogonal pattern's tile starts, in texels.
    fn ortho_tile(pattern: usize) -> (usize, usize) {
        let (across, down) = EDGES_ORTHO[pattern];
        (across * AREA_MAX_DISTANCE, down * AREA_MAX_DISTANCE)
    }

    #[test]
    fn the_patterns_with_nothing_to_blend_are_empty() {
        for pattern in [0, 5, 10, 15] {
            let (x0, y0) = ortho_tile(pattern);
            for x in x0..x0 + AREA_MAX_DISTANCE {
                for y in y0..y0 + AREA_MAX_DISTANCE {
                    assert_eq!(area_entry(x, y), [0, 0], "pattern {pattern} at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn an_l_pattern_and_its_mirror_read_the_same_area() {
        // `.------` at `left, right` is `------.` at `right, left`, and the
        // same for the two Ls that open the other way.
        for left in 0..AREA_MAX_DISTANCE {
            for right in 0..AREA_MAX_DISTANCE {
                let (x1, y1) = ortho_tile(1);
                let (x2, y2) = ortho_tile(2);
                assert_eq!(
                    area_entry(x1 + left, y1 + right),
                    area_entry(x2 + right, y2 + left),
                    "patterns 1 and 2 at {left}, {right}"
                );
                let (x4, y4) = ortho_tile(4);
                let (x8, y8) = ortho_tile(8);
                assert_eq!(
                    area_entry(x4 + left, y4 + right),
                    area_entry(x8 + right, y8 + left),
                    "patterns 4 and 8 at {left}, {right}"
                );
            }
        }
    }

    #[test]
    fn the_two_us_are_each_other_upside_down() {
        // `.------.` blends below the line where `` `------´ `` blends above.
        for left in 0..AREA_MAX_DISTANCE {
            for right in 0..AREA_MAX_DISTANCE {
                let (x3, y3) = ortho_tile(3);
                let (x12, y12) = ortho_tile(12);
                let [below, above] = area_entry(x3 + left, y3 + right);
                assert_eq!(
                    [above, below],
                    area_entry(x12 + left, y12 + right),
                    "patterns 3 and 12 at {left}, {right}"
                );
            }
        }
    }

    #[test]
    fn the_diagonal_block_is_filled_and_bounded() {
        let mut filled = 0;
        for x in ORTHO_BLOCK..AREA_WIDTH {
            for y in 0..AREA_HEIGHT {
                let [below, above] = area_entry(x, y);
                assert!(
                    usize::from(below) + usize::from(above) <= 255,
                    "({x}, {y}) blends more than a pixel"
                );
                filled += usize::from(below != 0 || above != 0);
            }
        }
        // Every diagonal pattern reaches its whole tile; the exact count is
        // the reference's business, but a block that is mostly empty was
        // assembled at the wrong place.
        assert!(
            filled > DIAG_BLOCK * DIAG_BLOCK / 2,
            "only {filled} diagonal texels are non-zero"
        );
    }
}
