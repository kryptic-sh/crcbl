#!/usr/bin/env bash
# Run the alcove binary once per occlusion flag and read the frames back to say
# what each flag actually presented.
#
#   CRCBL_GPU=vk apps/alcove/tests/run-alcove-views.sh
#
# # What this is for
#
# The flags were covered from the argument to the console cell and stopped
# there: `apps/alcove/src/args.rs` has tests that each of them parses and that
# `Options::apply` writes the cell, and `apps/alcove/tests/golden.rs` draws every
# one of those pictures from a fixture it builds itself. Between them sat the
# half nothing asked — that the frame a *flagged run of the shipped binary*
# presents is the picture the flag names. Every route the parse test covers would
# go on passing with the flag wired to a cell the renderer no longer reads.
#
# So this is the outside view: the command line a person types, and a reading off
# the PNG that came out of it. Five flags are held that way here — `--bent-view`
# and `--ao-view` name the picture, `--no-ao` takes the occlusion pass out from
# under it, `--technique` names the gather that fills it, and `--split` runs that
# gather against the shipped one down the frame — and each is read through the
# picture its own effect is visible in.
#
# # Why it drives the binary and not a suite
#
# `apps/alcove/tests/golden.rs` reaches `ForwardRenderer` directly and sets the
# view by hand, which is the only way to compare four arms of one frame — and it
# is also why it cannot answer this question: a fixture that sets the switch
# itself never touches the argument parser, the console cell, or `crate::app`'s
# start-up order. `crcbl::args::Common::with_screenshot` is what makes the frame
# reachable from outside, and `--screenshot` turns `--headless` on by itself, so
# nothing here opens a window.
#
# # Why the extent is the goldens'
#
# 256x192 is `apps/alcove/tests/golden.rs`'s `EXTENT`, and the reading below is
# placed at that suite's own projection of `apps/alcove/src/court.rs`'s
# `OPEN_FLOOR` through `court::fixed_camera` — pixel (49, 146), averaged over the
# block its `BLOCK` half-extents give there. Running at any other size would put
# the reading somewhere nobody measured. The bent, ao and shaded frames this
# script writes read identically to the three checked-in goldens at that block,
# which is the cross-check that the pixel is the one the suite means. The seam
# arm's bleed band is that suite's `SEAM_BLEED` for the same reason: it is a
# texel count rather than a fraction of the frame, so it holds at this extent.
#
# # Why the debug overlay is turned off
#
# It is not tidiness. At 256x192 the panel's text covers the left half of the
# frame, open floor included: the first run of this harness read [98, 127, 107]
# there and the picture underneath was perfect. `--no-debug-overlay` is what puts
# the court back under the reading.
#
# # ENVIRONMENT
#
#   CRCBL_GPU                      Which backend draws. Required; no default.
#   CRCBL_VK_ICD                   Which Vulkan driver, when `CRCBL_GPU=vk`.
#   CRCBL_ALCOVE_VIEWS_SELF_TEST   Break one claim on purpose, to watch it go
#                                  red. `no-bent-flag`, `no-ao-flag`,
#                                  `no-technique-flag` and `no-split-flag` each
#                                  draw one arm with the flag it is about
#                                  dropped; `off-floor` reads the block off the
#                                  open floor and onto a vertical surface. See
#                                  "How it was shown to fail".
#
# # What was measured
#
# Three runs per arm on each of this workstation's two Vulkan drivers — radv (AMD
# Radeon RX 7900 XTX, RADV NAVI31) and lavapipe (llvmpipe, LLVM 22.1.8), the one
# CI runs. Every run of an arm gave the same numbers, so each column below is
# three identical readings. The two view arms and their control:
#
# ```text
# arm            drift from the encoded +Y normal      widest channel spread
#                     radv        lavapipe             radv      lavapipe
# --bent-view         0.48            0.48              194           195
# --ao-view          67.48           67.48                0             0
# no flag            44.48           44.48               10            10
# ```
#
# "Drift" is the furthest channel of the open-floor block from the geometric
# normal encoded `n * 0.5 + 0.5` and put through the swapchain's sRGB encode,
# which is `apps/alcove/tests/golden.rs`'s open-floor claim in its own terms.
# "Spread" is `max(r,g,b) - min(r,g,b)` at the worst pixel of the whole frame,
# which is what "the picture is grey" means when it is a claim and not an
# impression.
#
# The three bounds come out of those two columns:
#
#   * the bent arm's drift is held under 2.5, which is that suite's own
#     `OPEN_FLOOR_BENT_TOLERANCE` and five times the 0.48 measured here. The
#     nearest thing it has to reject is the shaded frame at 44.48;
#   * a grey picture's spread is held at or under 1, on the reasoning that suite's
#     `SENTINEL_GREY_TOLERANCE` gives: one code is a differing sRGB rounding on
#     some other driver and nothing more. Measured 0, and the nearest thing it has
#     to reject is the shaded frame at 10;
#   * a picture that is not grey must spread at least 5, halfway between that 0
#     and the 10 the thinnest colour arm measured.
#
# The three arms added on 2026-09-05 are each read through a different statistic,
# because each of their flags moves a different thing. Same two drivers, three
# runs per arm, every run of an arm identical:
#
# ```text
# reading                                          radv    lavapipe   flag it is for
# --ao-view --no-ao, darkest pixel               255.00      255.00   --no-ao
# --ao-view, darkest pixel                       184.00      184.00     (its control)
# --bent-view --technique hemisphere,              0.16        0.16   --technique
#   codes off the sentinel grey
# --bent-view, the same                           67.16       67.16     (its control)
# --bent-view --technique hemisphere --split,     0.0000      0.0000   --split
#   worst column residue against its own side
# the same, thinnest column disagreement         21.8368     21.8906     (its control)
#   with the reference the other side read
# ```
#
# "Darkest" is the mean of a pixel's three channels at the darkest pixel of the
# whole frame — the statistic the occlusion channel's picture has, since a block
# on open floor reads 255 whether or not the pass ran. "The sentinel grey" is
# what `crcbl::shaders::ssao::BENT_NORMAL_NONE` encodes to, which is the picture
# a gather with no bisector to accumulate draws. The seam's two numbers are
# `golden.rs`'s `column_difference` in this reader's terms, over every column
# outside the bleed band.
#
# Their bounds come out of those pairs:
#
#   * the occlusion channel with `--no-ao` is held at or above 254 at its darkest
#     pixel: `crcbl_render::forward` binds a 1x1 white where the pass would be, so
#     the picture is white everywhere and the one code is another driver's sRGB
#     rounding. Measured 255.00, and what it has to reject is the same picture
#     with the pass in at 184.00;
#   * that control is held the other way at or under 220, halfway between the two;
#   * the hemisphere arm's block is held within 1 code of the sentinel grey —
#     `golden.rs`'s `SENTINEL_GREY_TOLERANCE`, and its whole-frame spread within
#     the same 1 — and at least 28 codes from the block the same command draws
#     without the flag, which is that suite's `SHIPPED_OFF_SENTINEL` and about
#     half the 67.00 measured;
#   * the seamed frame's columns are held *exactly* equal to the reference for
#     their own side, as `golden.rs` holds its own seam: measured 0.0000 on both
#     drivers, and the two frames being compared come off one driver in one run,
#     so there is no rounding to admit. The disagreement with the other side's
#     reference is held above 10, half the thinnest measured, which is what stops
#     the equality being an equality of two identical pictures.
#
# # How it was shown to fail
#
# Five of them have a switch, because they are the ones a reader will want to
# re-run:
#
#   CRCBL_GPU=vk CRCBL_ALCOVE_VIEWS_SELF_TEST=no-bent-flag \
#     apps/alcove/tests/run-alcove-views.sh
#   CRCBL_GPU=vk CRCBL_ALCOVE_VIEWS_SELF_TEST=off-floor \
#     apps/alcove/tests/run-alcove-views.sh
#   CRCBL_GPU=vk CRCBL_ALCOVE_VIEWS_SELF_TEST=no-ao-flag \
#     apps/alcove/tests/run-alcove-views.sh
#   CRCBL_GPU=vk CRCBL_ALCOVE_VIEWS_SELF_TEST=no-technique-flag \
#     apps/alcove/tests/run-alcove-views.sh
#   CRCBL_GPU=vk CRCBL_ALCOVE_VIEWS_SELF_TEST=no-split-flag \
#     apps/alcove/tests/run-alcove-views.sh
#
# The first drops `--bent-view` from the arm that is meant to carry it, which is
# what a flag that had stopped reaching `crcbl::debug_view` would leave: the
# shaded court is drawn instead, and the open-floor claim reports
# `--bent-view draws [232.00, 230.00, 227.00] on the open floor … 44.48 codes apart, past 2.5`.
# The second moves the reading 106 rows up, to (49, 40): a vertical surface at
# the back of the court, where the bent picture reports a direction of about
# `(0.54, 0.19, 0.80)` — pointing out of the frame rather than up — and the same
# claim reports `63.80 codes apart, past 2.5`. A claim that survived a reading
# placed anywhere would be no claim about the open floor at all.
#
# The last three drop the flag their own arm is about, which is what a flag that
# had stopped reaching the console cell would leave. Each was run on both drivers
# on 2026-09-05:
#
#   * `no-ao-flag` draws the occlusion channel with the pass left in, and the
#     white picture is gone:
#     `--no-ao draws 184.00/255 at the darkest pixel of the occlusion channel (74, 96), short of 254.0`;
#   * `no-technique-flag` draws the bent picture on the shipped gather, so all
#     three of that arm's claims go red together:
#     `--technique hemisphere spreads 194 codes across its channels at (237, 0), past 1`,
#     then `[188.00, 255.00, 188.00] … 67.16 codes apart, past 1.0` off the
#     sentinel grey, and `0.00 codes apart, short of 28.0` from the block the
#     same command draws without the flag. Lavapipe spreads 195, at (236, 5);
#   * `no-split-flag` runs the hemisphere gather over the whole frame, so the far
#     side of the seam is the wrong picture:
#     `column 254 of the --split frame differs from the whole-frame bent run by 41.6076/255`
#     — column 255 at 41.5573 on lavapipe, the worst column of the 233 rather
#     than a column picked out.
#
# The other four were reddened by giving one arm another arm's flag, on a copy
# of this script, which is what a parser that had crossed two flags would do:
#
#   * the ao arm drawn `--bent-view`:
#     `--ao-view spreads 194 codes across its channels at (237, 0), past 1`;
#   * the shaded arm drawn `--ao-view`:
#     `a run with no view flag spreads 0 codes at (0, 0), short of 5`;
#   * the shaded arm drawn `--bent-view`:
#     `a run with no view flag draws [188.00, 255.00, 188.00] on the open floor, … short of 20.0`;
#   * and the bent arm drawn `--ao-view`, which is the sentinel picture arriving
#     where a direction was wanted:
#     `--bent-view spreads 0 codes at its widest pixel (0, 0), short of 5`.

set -euo pipefail

APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${APP_DIR}/../.." && pwd)"

# shellcheck source=crates/crcbl-vk/tests/vulkan-icd.sh
source "${REPO_ROOT}/crates/crcbl-vk/tests/vulkan-icd.sh"
crcbl_pin_vk_icd "alcove views"

# shellcheck source=tools/vk-validation-log.sh
source "${REPO_ROOT}/tools/vk-validation-log.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
alcove views: CRCBL_GPU is not set, so nothing would pin the backend and a
  fallback would pass. Name one:

    CRCBL_GPU=vk   $0     # Vulkan
    CRCBL_GPU=mtl  $0     # Metal, macOS
    CRCBL_GPU=dx12 $0     # Direct3D 12, Windows
NOBACKEND
    exit 1
fi

# A vk run validates whatever the shell says: the check after each run reads what
# the layer said, and a `CRCBL_VK_VALIDATION=0` left over from profiling would
# hand it a log with no messenger in it — which it rejects, for the wrong reason.
if [ "$CRCBL_GPU" = vk ]; then
    export CRCBL_VK_VALIDATION=1
fi

SELF_TEST="${CRCBL_ALCOVE_VIEWS_SELF_TEST:-}"
case "$SELF_TEST" in
    '' | no-bent-flag | no-ao-flag | no-technique-flag | no-split-flag | off-floor) ;;
    *)
        echo "alcove views: CRCBL_ALCOVE_VIEWS_SELF_TEST=$SELF_TEST is not one of" >&2
        echo "  'no-bent-flag', 'no-ao-flag', 'no-technique-flag'," >&2
        echo "  'no-split-flag' or 'off-floor'. A misspelt sabotage that ran the" >&2
        echo "  ordinary check would report a green run as a red one caught." >&2
        exit 1
        ;;
esac

cd "$REPO_ROOT"

# The extent every reading below was placed at — `apps/alcove/tests/golden.rs`'s
# `EXTENT`.
EXTENT=256x192
# `court::OPEN_FLOOR` through `court::fixed_camera` at that extent, and the half
# extent of the block that suite averages over there.
OPEN_FLOOR_X=49
OPEN_FLOOR_Y=146
BLOCK_HALF=2
# How many frames a run presents before its screenshot is taken. Past start-up,
# and the readings in the sweep above were the same number on both drivers at
# this count.
FRAMES=30

if [ "$SELF_TEST" = off-floor ]; then
    # A vertical surface at the back of the court, 106 rows above the open
    # floor: a point where the bent direction points out of the frame rather
    # than up.
    OPEN_FLOOR_Y=40
    echo "alcove views: SELF TEST — the reading moves to" \
        "($OPEN_FLOOR_X, $OPEN_FLOOR_Y), off the open floor"
fi

# Where the three frames land, beside the review frames `golden.rs` writes.
SHOTS="${REPO_ROOT}/target/alcove/views"
mkdir -p "$SHOTS"

LOG="$(mktemp -t crcbl-alcove-views.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `draw <name> <flag…>` — one run of the binary, into `$SHOTS/<name>.png`.
draw() {
    local name="$1"
    shift
    local shot="${SHOTS}/${name}.png"

    # Removed first: a run that wrote no frame at all must fail here rather than
    # hand the reading a picture left over from the last one.
    rm -f "$shot"

    echo "alcove views: drawing $name${1:+ with $*}"
    if ! cargo run --locked --quiet --package alcove -- \
        --headless --backend "$CRCBL_GPU" --frames "$FRAMES" --size "$EXTENT" \
        --no-debug-overlay --screenshot "$shot" "$@" >"$LOG" 2>&1; then
        echo "alcove views: the $name run failed on $CRCBL_GPU" >&2
        cat "$LOG" >&2
        exit 1
    fi

    if [ ! -f "$shot" ]; then
        echo "alcove views: the $name run exited 0 and wrote no frame to $shot." >&2
        echo "  --screenshot is a flag this binary has only because its Common" >&2
        echo "  said with_screenshot; a run that consumed it and wrote nothing" >&2
        echo "  would leave every reading below to be made off a stale file." >&2
        exit 1
    fi

    # What the validation layer said. A violation reaches `crcbl_core::log::error!`
    # and the process still exits 0, so without this the run advertises that it is
    # validating and cannot fail because of it.
    if [ "$CRCBL_GPU" = vk ] \
        && ! crcbl_validation_saw_nothing "$LOG" "the alcove $name run"; then
        exit 1
    fi
}

BENT_ARGS=(--bent-view)
if [ "$SELF_TEST" = no-bent-flag ]; then
    BENT_ARGS=()
    echo "alcove views: SELF TEST — the bent arm draws with --bent-view dropped"
fi

# **`--no-ao` is read through the occlusion channel's own picture**, because
# that is where taking the pass out has an absolute answer: `crcbl_render::forward`
# binds a 1x1 white where the chain would be, so the channel a flagged run draws
# is white at every pixel. The shaded court would show the same thing as a few
# codes at two blocks, which is a reading the driver's rounding is in.
NO_AO_ARGS=(--ao-view --no-ao)
if [ "$SELF_TEST" = no-ao-flag ]; then
    NO_AO_ARGS=(--ao-view)
    echo "alcove views: SELF TEST — the no-ao arm draws with --no-ao dropped"
fi

# **`--technique` is read through the bent picture**, because the two gathers
# differ there by the whole of it rather than by a few codes:
# `crates/crcbl-shaders/shaders/ssao_hemisphere.slang` sums depth comparisons
# instead of sweeping a horizon, so it has no bisector to accumulate and writes
# `crcbl::shaders::ssao::BENT_NORMAL_NONE` beside every scalar — one flat grey,
# against the direction the shipped gather draws. `golden.rs`'s
# `the_bent_direction_view_draws_the_sentinel_grey_where_no_direction_was_gathered`
# is the same picture from the inside.
HEMISPHERE_ARGS=(--bent-view --technique hemisphere)
if [ "$SELF_TEST" = no-technique-flag ]; then
    HEMISPHERE_ARGS=(--bent-view)
    echo "alcove views: SELF TEST — the hemisphere arm draws with --technique dropped"
fi

# And the seam runs that gather against the shipped one in one frame, which is
# the only arm here whose reading is a column rather than a pixel.
SEAM_ARGS=(--bent-view --technique hemisphere --split)
if [ "$SELF_TEST" = no-split-flag ]; then
    SEAM_ARGS=(--bent-view --technique hemisphere)
    echo "alcove views: SELF TEST — the seam arm draws with --split dropped"
fi

draw bent "${BENT_ARGS[@]}"
draw ao --ao-view
draw shaded
draw no-ao "${NO_AO_ARGS[@]}"
draw hemisphere "${HEMISPHERE_ARGS[@]}"
draw seam "${SEAM_ARGS[@]}"

# The readings, and every claim made from them.
#
# Python's standard library rather than a crate: the three frames are 8-bit PNGs
# and `zlib` decompresses them, so this needs nothing installed that a runner
# does not already have. There is no PNG *reader* in this workspace that a shell
# script can call — `crcbl screenshot` writes one and `crcbl crpix` packs them —
# and adding a dependency to read four hundred lines of pixels is not the trade.
python3 - "$SHOTS" "$OPEN_FLOOR_X" "$OPEN_FLOOR_Y" "$BLOCK_HALF" <<'MEASURE'
import struct
import sys
import zlib

SHOTS = sys.argv[1]
CX, CY, HALF = int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])

# How far the bent arm's open-floor block may sit from the encoded geometric
# normal, in 0-255 codes on its furthest channel. `golden.rs`'s
# OPEN_FLOOR_BENT_TOLERANCE, and the measurement is in this script's header.
BENT_DRIFT_MOST = 2.5
# How far a grey picture's worst pixel may spread across its channels. One code,
# which is a differing sRGB rounding on some other driver and nothing more.
GREY_SPREAD_MOST = 1
# How far a picture that is *not* grey must spread there.
COLOUR_SPREAD_LEAST = 5
# How far the shaded arm's open-floor block must stand off the encoded normal,
# on the same furthest channel.
SHADED_DRIFT_LEAST = 20.0
# How dark the darkest pixel of the occlusion channel may be with the pass out,
# out of 255. White everywhere, and the one code is another driver's rounding.
NO_AO_WHITE_LEAST = 254.0
# How dark that same pixel must be with the pass in — halfway to the reading
# above, and what says the claim above is about a pass that darkened something.
AO_DARKEST_MOST = 220.0
# The byte an `Rgba8Unorm` bent channel holds where the gather reported no
# direction: `crcbl::shaders::ssao::BENT_NORMAL_NONE`, which the bent view passes
# through unchanged.
BENT_NORMAL_NONE = 128
# How far the hemisphere arm's block may sit from what that byte encodes to.
# `golden.rs`'s SENTINEL_GREY_TOLERANCE, and its reasoning: one code is a
# differing sRGB rounding on some other driver and nothing more.
SENTINEL_GREY_MOST = 1.0
# How far that arm's block must stand from the block the same command draws with
# no `--technique` at all. `golden.rs`'s SHIPPED_OFF_SENTINEL.
TECHNIQUE_MOVES_THE_BLOCK_BY = 28.0
# How many pixels either side of the seam are not compared. Not slack: the blur
# and the depth-aware upsample run over the whole target where `crcbl_render::split`
# divides the gather alone, so those columns belong to neither reference frame.
# `golden.rs`'s SEAM_BLEED, and it is a texel count rather than a fraction of the
# frame.
SEAM_BLEED = 12
# How far a column outside that band must sit from the reference the *other* side
# of the seam read, in mean 0-255 codes. Half the thinnest measured.
SEAM_SIDES_DIFFER_BY = 10.0


def read_png(path):
    """The pixels of an 8-bit non-interlaced RGB or RGBA PNG, row-major."""
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"alcove views: {path} is not a PNG")
    at, idat, width, height, colour = 8, [], None, None, None
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at : at + 8])
        at += 8
        body = data[at : at + length]
        at += length + 4
        if kind == b"IHDR":
            fields = struct.unpack(">IIBBBBB", body)
            width, height, depth, colour, _, _, interlace = fields
            if depth != 8 or colour not in (2, 6) or interlace != 0:
                raise SystemExit(
                    f"alcove views: {path} is depth {depth}, colour type {colour},"
                    f" interlace {interlace}; this reader handles 8-bit RGB(A) only"
                )
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    raw = zlib.decompress(b"".join(idat))
    step = 3 if colour == 2 else 4
    stride = width * step
    pixels = bytearray(height * stride)
    previous = bytearray(stride)
    at = 0
    for row in range(height):
        # PNG's five per-row filters, RFC 2083 §6. Each reconstructs a byte from
        # its left neighbour `a`, the byte above `b`, and the one above-left `c`.
        kind = raw[at]
        at += 1
        line = bytearray(raw[at : at + stride])
        at += stride
        if kind == 1:
            for i in range(step, stride):
                line[i] = (line[i] + line[i - step]) & 255
        elif kind == 2:
            for i in range(stride):
                line[i] = (line[i] + previous[i]) & 255
        elif kind == 3:
            for i in range(stride):
                a = line[i - step] if i >= step else 0
                line[i] = (line[i] + ((a + previous[i]) >> 1)) & 255
        elif kind == 4:
            for i in range(stride):
                a = line[i - step] if i >= step else 0
                b = previous[i]
                c = previous[i - step] if i >= step else 0
                guess = a + b - c
                da, db, dc = abs(guess - a), abs(guess - b), abs(guess - c)
                if da <= db and da <= dc:
                    line[i] = (line[i] + a) & 255
                elif db <= dc:
                    line[i] = (line[i] + b) & 255
                else:
                    line[i] = (line[i] + c) & 255
        elif kind != 0:
            raise SystemExit(f"alcove views: {path} row {row} has filter {kind}")
        pixels[row * stride : (row + 1) * stride] = line
        previous = line
    return width, height, step, bytes(pixels)


def srgb_encode(value):
    """`value` in linear light, as the swapchain's sRGB encode writes it, out of
    255. IEC 61966-2-1's transfer function, which is what the Vulkan
    specification's sRGB conversion is."""
    if value <= 0.0031308:
        return value * 12.92 * 255.0
    return (1.055 * value ** (1 / 2.4) - 0.055) * 255.0


class Frame:
    def __init__(self, name):
        self.name = name
        self.path = f"{SHOTS}/{name}.png"
        self.width, self.height, self.step, self.pixels = read_png(self.path)
        step, pixels = self.step, self.pixels

        total, count = [0.0, 0.0, 0.0], 0
        for y in range(max(0, CY - HALF), min(self.height - 1, CY + HALF) + 1):
            for x in range(max(0, CX - HALF), min(self.width - 1, CX + HALF) + 1):
                at = (y * self.width + x) * step
                for channel in range(3):
                    total[channel] += pixels[at + channel]
                count += 1
        if count == 0:
            raise SystemExit(f"alcove views: ({CX}, {CY}) is outside {self.path}")
        self.block = [sum_ / count for sum_ in total]

        self.spread, self.worst = 0, (0, 0)
        self.darkest, self.darkest_at = 255.0, (0, 0)
        for y in range(self.height):
            for x in range(self.width):
                at = (y * self.width + x) * step
                r, g, b = pixels[at], pixels[at + 1], pixels[at + 2]
                here = max(r, g, b) - min(r, g, b)
                if here > self.spread:
                    self.spread, self.worst = here, (x, y)
                mean = (r + g + b) / 3.0
                if mean < self.darkest:
                    self.darkest, self.darkest_at = mean, (x, y)

    def readings(self):
        return "[%.2f, %.2f, %.2f]" % tuple(self.block)


def column_difference(a, b, x):
    """Mean absolute difference down column `x` of two frames, out of 255.

    `golden.rs`'s own `column_difference` in this reader's terms, and the unit
    its seam claim is stated in: a column either agrees with a reference frame or
    it does not, and reducing it to one number is what makes "to the column" a
    thing to assert."""
    total = 0.0
    for y in range(a.height):
        at_a, at_b = (y * a.width + x) * a.step, (y * b.width + x) * b.step
        mean_a = sum(a.pixels[at_a + channel] for channel in range(3)) / 3.0
        mean_b = sum(b.pixels[at_b + channel] for channel in range(3)) / 3.0
        total += abs(mean_a - mean_b)
    return total / a.height


# `+Y` — the open floor's own normal — encoded `n * 0.5 + 0.5` and put through
# the swapchain's sRGB encode, which is `golden.rs`'s open-floor claim verbatim.
UP = [srgb_encode(0.5), srgb_encode(1.0), srgb_encode(0.5)]
UP_TEXT = "[%.2f, %.2f, %.2f]" % tuple(UP)
# The sentinel byte through the same encode — the picture a gather with no
# bisector to accumulate draws, everywhere.
GREY = srgb_encode(BENT_NORMAL_NONE / 255.0)

bent, ao, shaded = Frame("bent"), Frame("ao"), Frame("shaded")
no_ao, hemisphere, seamed = Frame("no-ao"), Frame("hemisphere"), Frame("seam")


def drift(frame):
    return max(abs(frame.block[at] - UP[at]) for at in range(3))


faults = []

print(
    f"alcove views: --bent-view draws {bent.readings()} at ({CX}, {CY}) against the"
    f" geometric normal {UP_TEXT}, {drift(bent):.2f} codes apart, and spreads"
    f" {bent.spread} at {bent.worst}"
)
if drift(bent) > BENT_DRIFT_MOST:
    faults.append(
        f"--bent-view draws {bent.readings()} on the open floor and the floor's own"
        f" normal encodes to {UP_TEXT} — {drift(bent):.2f} codes apart, past"
        f" {BENT_DRIFT_MOST}. Nothing is within the occlusion radius of that point, so"
        " the average unblocked direction there is the normal itself"
    )
if bent.spread < COLOUR_SPREAD_LEAST:
    faults.append(
        f"--bent-view spreads {bent.spread} codes at its widest pixel {bent.worst},"
        f" short of {COLOUR_SPREAD_LEAST} — that is a grey picture, which is what the"
        " bent view draws where no direction was gathered at all"
    )

print(
    f"alcove views: --ao-view draws {ao.readings()} at ({CX}, {CY}) and spreads"
    f" {ao.spread} at {ao.worst}"
)
if ao.spread > GREY_SPREAD_MOST:
    faults.append(
        f"--ao-view spreads {ao.spread} codes across its channels at {ao.worst}, past"
        f" {GREY_SPREAD_MOST}. The occlusion channel is one scalar, so the picture of"
        " it is grey at every pixel"
    )

print(
    f"alcove views: no view flag draws {shaded.readings()} at ({CX}, {CY}),"
    f" {drift(shaded):.2f} codes off the geometric normal, and spreads"
    f" {shaded.spread} at {shaded.worst}"
)
if drift(shaded) < SHADED_DRIFT_LEAST:
    faults.append(
        f"a run with no view flag draws {shaded.readings()} on the open floor,"
        f" {drift(shaded):.2f} codes off the encoded normal and short of"
        f" {SHADED_DRIFT_LEAST} — that is the bent picture, from a run that asked for"
        " the shaded court"
    )
if shaded.spread < COLOUR_SPREAD_LEAST:
    faults.append(
        f"a run with no view flag spreads {shaded.spread} codes at {shaded.worst},"
        f" short of {COLOUR_SPREAD_LEAST} — that is a grey picture, from a run that"
        " asked for the shaded court"
    )

print(
    f"alcove views: --no-ao draws the occlusion channel at {no_ao.darkest:.2f}/255 at"
    f" its darkest pixel {no_ao.darkest_at}, against {ao.darkest:.2f} at {ao.darkest_at}"
    " with the pass in"
)
if no_ao.darkest < NO_AO_WHITE_LEAST:
    faults.append(
        f"--no-ao draws {no_ao.darkest:.2f}/255 at the darkest pixel of the occlusion"
        f" channel {no_ao.darkest_at}, short of {NO_AO_WHITE_LEAST}. With the pass out the"
        " renderer binds its 1x1 white in place of the chain, so the picture of that"
        " channel is white at every pixel"
    )
if ao.darkest > AO_DARKEST_MOST:
    faults.append(
        f"the occlusion channel with the pass *in* reads {ao.darkest:.2f}/255 at its darkest"
        f" pixel {ao.darkest_at}, past {AO_DARKEST_MOST} — so it is the white picture too and"
        " the claim above is one about a pass that darkened nothing"
    )

sentinel = max(abs(hemisphere.block[at] - GREY) for at in range(3))
moved = max(abs(hemisphere.block[at] - bent.block[at]) for at in range(3))
print(
    f"alcove views: --technique hemisphere draws {hemisphere.readings()} at ({CX}, {CY}),"
    f" {sentinel:.2f} codes off the sentinel grey {GREY:.2f} and {moved:.2f} from the block"
    f" the same command draws without it, and spreads {hemisphere.spread} at"
    f" {hemisphere.worst}"
)
if hemisphere.spread > GREY_SPREAD_MOST:
    faults.append(
        f"--technique hemisphere spreads {hemisphere.spread} codes across its channels at"
        f" {hemisphere.worst}, past {GREY_SPREAD_MOST}. That gather sums depth comparisons"
        " instead of sweeping a horizon, so it has no bisector to report and its bent picture"
        " is one flat grey"
    )
if sentinel > SENTINEL_GREY_MOST:
    faults.append(
        f"--technique hemisphere draws {hemisphere.readings()} on the open floor and the byte"
        f" a gather with no direction writes encodes to {GREY:.2f} — {sentinel:.2f} codes"
        f" apart, past {SENTINEL_GREY_MOST}"
    )
if moved < TECHNIQUE_MOVES_THE_BLOCK_BY:
    faults.append(
        f"--technique hemisphere draws {hemisphere.readings()} on the open floor and the same"
        f" command with no --technique draws {bent.readings()} — {moved:.2f} codes apart,"
        f" short of {TECHNIQUE_MOVES_THE_BLOCK_BY}. The two gathers are meant to draw"
        " different pictures there, so the flag reached neither the console cell nor the pass"
    )

# The seam: every column of the seamed frame against **both** whole-frame runs.
# The near side is the gather `--technique` named and the far side is what ships,
# so each column has one reference it must equal and one it must not.
seam_at = seamed.width // 2
columns, worst, thinnest = 0, None, None
for x in range(seamed.width):
    if abs(x - seam_at) < SEAM_BLEED:
        continue
    near_side = x < seam_at
    mine, theirs = (hemisphere, bent) if near_side else (bent, hemisphere)
    residue = column_difference(seamed, mine, x)
    if worst is None or residue > worst[0]:
        worst = (residue, x, "left" if near_side else "right", mine.name)
    disagreement = column_difference(seamed, theirs, x)
    thinnest = disagreement if thinnest is None else min(thinnest, disagreement)
    columns += 1
if worst is None or thinnest is None:
    faults.append(
        f"the bleed band swallowed the whole {seamed.width}-column frame, so the seam claim"
        " compared nothing"
    )
else:
    print(
        f"alcove views: --split compares {columns} columns either side of {seam_at} — worst"
        f" {worst[0]:.4f} against the gather that side ran, thinnest {thinnest:.4f} against"
        " the other one"
    )
    if worst[0] > 0:
        residue, x, side, name = worst
        faults.append(
            f"column {x} of the --split frame differs from the whole-frame {name} run by"
            f" {residue:.4f}/255. With the seam at the centre the {side} of the frame is meant"
            " to be that gather and nothing else"
        )
    if thinnest < SEAM_SIDES_DIFFER_BY:
        faults.append(
            f"some column of the --split frame outside the bleed band sits {thinnest:.4f}/255"
            f" from the gather the *other* side ran, short of {SEAM_SIDES_DIFFER_BY} — the two"
            " sides cannot be told apart there, so the equality above proves nothing"
        )

if faults:
    # Flushed first: stderr is unbuffered and stdout is not when this is piped,
    # so without it the diagnosis prints above the readings it was made from.
    sys.stdout.flush()
    for fault in faults:
        print(f"alcove views: {fault}", file=sys.stderr)
    raise SystemExit(1)
MEASURE

echo "alcove views: each flag presented its own picture on $CRCBL_GPU;" \
    "frames in $SHOTS"
