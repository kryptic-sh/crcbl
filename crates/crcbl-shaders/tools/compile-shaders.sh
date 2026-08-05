#!/usr/bin/env bash
# Compile `shaders/*.slang` to the committed SPIR-V in `spirv/`, the WGSL in
# `wgsl/`, the MSL in `msl/` and the DXIL in `dxil/`, and rewrite
# `spirv/manifest.txt`.
#
#   crates/crcbl-shaders/tools/compile-shaders.sh            # regenerate
#   crates/crcbl-shaders/tools/compile-shaders.sh --check     # verify only
#
# # Why the artifacts are committed
#
# `docs/plan/02-vulkan-backend.md` §2.3 chooses Slang, and its own risk list
# names the escape hatch this script implements: "Slang toolchain friction in
# build.rs. Fallback: check in compiled SPIR-V alongside sources until the
# toolchain story is smooth."
#
# The consequence is that **`cargo build` needs no shader compiler at all** —
# not on a contributor's machine, not on the macOS and Windows CI legs, not in
# the `test (linux)` job. `build.rs` only ever *verifies*, and it verifies with
# SHA-256 rather than by recompiling, so the check is identical everywhere.
#
# # --check is the anti-rot gate
#
# `--check` recompiles every source with the pinned `slangc` and demands the
# result be byte-for-byte what is committed. It runs in CI (which installs the
# pinned compiler; see `.github/workflows/ci.yml`), so "someone edited the
# `.slang` and forgot to regenerate" is caught by a machine that is not the
# author's. The version pin is load-bearing: two Slang releases legitimately
# emit different SPIR-V for identical source, so an unpinned byte comparison
# would fail for a reason that is not drift.
#
# # DXIL is compiled in two steps, and its compiler is pinned by path
#
# Slang's own `-target dxil` does not compile it: Slang shells out to a
# downstream `dxc` and inherits whichever one it finds, so the artifact would be
# produced by an ambient compiler that no pin describes. This script therefore
# goes `slangc -target hlsl` and then `dxc` explicitly, per entry point, with
# the `dxc` named by `CRCBL_DXC`.
#
# **`CRCBL_DXC` never falls back to PATH**, unlike `CRCBL_SLANGC`, and that is
# not symmetry that was overlooked. Distributions ship `dxc` builds from the
# Shader Model 6.10 preview branch — Arch's `directx-shader-compiler`
# 1.10.2605.24 is one, and it aborts with an LLVM internal assertion on a
# four-line shader. A fallback would find it first and there would be nothing in
# the output to say so, which is the one failure mode a pin exists to prevent.
#
# `dxc` reports the version of the `libdxcompiler.so` it *loaded*, not of the
# executable, so the version check below is the right check: the stable
# distribution's launcher finds its own library through an RPATH, and a launcher
# that picked up a system library would report that library's version and be
# refused here.
#
# ENVIRONMENT
#   CRCBL_SLANGC   Path to `slangc`. Default: whatever is on PATH.
#   CRCBL_DXC      Path to `dxc`. **Required**, with no PATH fallback.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Pinned in exactly one place, and read by CI. Bumping it is a deliberate act
# that re-blesses every artifact in the same commit — which is also the moment
# to re-bless any golden image the new codegen moved.
SLANG_VERSION="2026.14"
# Vulkan 1.3 accepts SPIR-V 1.6; 1.5 is the floor every 1.3 driver has had
# since launch and nothing here needs a 1.6 instruction.
SLANG_PROFILE="spirv_1_5"

# The `microsoft/DirectXShaderCompiler` release the DXIL is built by, and the
# Linux asset inside it. CI reads both; see `.github/workflows/ci.yml`.
DXC_RELEASE="v1.9.2607"
DXC_ASSET="linux_dxc_2026_07_29.x86_x64.tar.gz"
# What `dxc --version` must report, with the loaded library's name stripped off
# the front. A *release tag* cannot be checked at run time — the binary does not
# know it — so this is the number the pin is actually enforced on.
DXC_VERSION="1.9(1-0d3ee6b5)(1.9.0.1)"

# Shader Model 6.6, and the whole backend rests on the choice.
#
# `crcbl-dx12` is specced around SM6.6 **dynamic resources** — `ResourceHeap`
# indexing, which is what makes a bindless renderer expressible on D3D12 — and
# WARP, the software rasteriser that is CI's only Windows executor, reports
# `HighestShaderModel=6.8` with `sm66-dynamic-resources=yes`. So 6.6 is the
# lowest model that keeps the feature the backend is designed around, and it
# leaves two minor versions of headroom on the one adapter that has to run it.
# Compiling at 6.8 would spend that headroom to gain nothing any shader here
# uses, and would exclude every driver capping out below it.
DXIL_MODEL="6_6"

CHECK_ONLY=0
if [ "${1:-}" = "--check" ]; then
    CHECK_ONLY=1
    shift
fi
if [ "$#" -gt 0 ]; then
    echo "crcbl shaders: unexpected argument '$1'; usage: compile-shaders.sh [--check]" >&2
    exit 2
fi

SLANGC="${CRCBL_SLANGC:-slangc}"
if ! command -v "$SLANGC" >/dev/null 2>&1; then
    cat >&2 <<EOF
crcbl shaders: no \`slangc\` found (looked for '$SLANGC').

This script is the only thing that needs one — \`cargo build\` does not, because
the compiled SPIR-V is committed. Install it only if you are editing a shader:

  curl -sL -o slang.tar.gz \\
    https://github.com/shader-slang/slang/releases/download/v${SLANG_VERSION}/slang-${SLANG_VERSION}-linux-x86_64-glibc-2.27.tar.gz
  mkdir -p ~/.local/slang && tar xzf slang.tar.gz -C ~/.local/slang
  export CRCBL_SLANGC=~/.local/slang/bin/slangc

Use exactly v${SLANG_VERSION}: the artifacts are compared byte-for-byte.
EOF
    exit 1
fi

FOUND_VERSION="$("$SLANGC" -v 2>&1 | tr -d '\r' | tail -1)"
if [ "$FOUND_VERSION" != "$SLANG_VERSION" ]; then
    echo "crcbl shaders: \`$SLANGC\` is version '$FOUND_VERSION', but the artifacts are" >&2
    echo "  pinned to '$SLANG_VERSION'. Different releases emit different SPIR-V for the" >&2
    echo "  same source, so this would produce spurious drift. Install the pinned one, or" >&2
    echo "  bump SLANG_VERSION here and re-bless every artifact and golden image together." >&2
    exit 1
fi

DXC="${CRCBL_DXC:-}"
if [ -z "$DXC" ] || ! command -v "$DXC" >/dev/null 2>&1; then
    cat >&2 <<EOF
crcbl shaders: CRCBL_DXC must name a \`dxc\` executable (it is '${DXC:-unset}').

There is deliberately **no PATH fallback** for this one. Distributions ship
Shader Model 6.10 *preview* builds of dxc — Arch's directx-shader-compiler
1.10.2605.24 is one — which abort with an LLVM internal assertion on a trivial
shader, and a fallback would find one silently. Install the pinned release:

  curl -sL -o dxc.tar.gz \\
    https://github.com/microsoft/DirectXShaderCompiler/releases/download/${DXC_RELEASE}/${DXC_ASSET}
  mkdir -p ~/.local/dxc && tar xzf dxc.tar.gz -C ~/.local/dxc --strip-components=1
  export CRCBL_DXC=~/.local/dxc/bin/dxc

The archive wraps everything in a directory named after the asset, which is what
\`--strip-components=1\` removes. It is around 492 MiB. \`bin/dxc\` finds its own
\`lib/libdxcompiler.so\` and \`lib/libdxil.so\` through an RPATH, so no
LD_LIBRARY_PATH is needed — and \`libdxil.so\` is what signs the container,
without which a real driver rejects the artifact.
EOF
    exit 1
fi

# `dxc --version` prints one line naming the library it loaded and that
# library's version: `libdxcompiler.so: 1.9(…)`. The prefix differs by platform,
# so only what follows the colon is compared.
FOUND_DXC="$("$DXC" --version 2>&1 | tr -d '\r' | head -1 | sed 's/^.*: //')"
if [ "$FOUND_DXC" != "$DXC_VERSION" ]; then
    echo "crcbl shaders: \`$DXC\` loaded a libdxcompiler reporting '$FOUND_DXC', but the" >&2
    echo "  artifacts are pinned to '$DXC_VERSION' (release $DXC_RELEASE). Different dxc" >&2
    echo "  releases emit different DXIL for the same source, and the preview builds" >&2
    echo "  distributions ship crash outright. Install the pinned one, or bump DXC_VERSION" >&2
    echo "  here and re-bless every artifact together." >&2
    exit 1
fi

cd "$CRATE_DIR"
mkdir -p spirv
mkdir -p wgsl
mkdir -p msl
mkdir -p dxil

WORK="$(mktemp -d -t crcbl-shaders.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT INT TERM

MANIFEST="$WORK/manifest.txt"
{
    echo "# Generated by tools/compile-shaders.sh — do not edit by hand."
    echo "#"
    echo "# \`build.rs\` fails the build when a source's SHA-256 stops matching the one"
    echo "# recorded here, which is how a shader edited without regenerating its artifact"
    echo "# is caught on a machine with no shader compiler at all."
    echo "slangc-version = $SLANG_VERSION"
    echo "target = $SLANG_PROFILE"
    echo "dxc-version = $DXC_VERSION"
    echo "dxil-model = $DXIL_MODEL"
} >"$MANIFEST"

# The DXIL profile prefix for one of `spirv-dis`' execution models, or nothing
# for a model no graphics or compute profile covers.
dxc_profile_prefix() {
    case "$1" in
    vertex) echo "vs" ;;
    fragment) echo "ps" ;;
    glcompute | compute) echo "cs" ;;
    *) echo "" ;;
    esac
}

# What `slangc -stage` calls one of `spirv-dis`' execution models.
slang_stage() {
    case "$1" in
    glcompute) echo "compute" ;;
    *) echo "$1" ;;
    esac
}

# Fails unless `$1` is a signed DXIL container.
#
# A `DxilContainerHeader` opens with the magic `DXBC` and a 16-byte digest, and
# the digest is **all zero until `libdxil.so` signs it**. An unsigned container
# compiles, hashes, and commits perfectly happily, and is then refused by every
# real D3D12 driver — WARP included — so this is checked on every artifact
# rather than spot-checked on one: a `dxc` that loaded no signing library
# produces a whole directory of them and nothing else in this script would
# notice.
require_signed_dxil() {
    local file="$1"
    local magic digest
    magic="$(dd if="$file" bs=1 count=4 2>/dev/null)"
    if [ "$magic" != "DXBC" ]; then
        echo "crcbl shaders: $file does not open with the DXBC container magic" >&2
        exit 1
    fi
    digest="$(dd if="$file" bs=1 skip=4 count=16 2>/dev/null | od -An -v -t x1 | tr -d ' \n')"
    if [ "$digest" = "00000000000000000000000000000000" ]; then
        echo "crcbl shaders: $file has an all-zero container digest, so it was never signed." >&2
        echo "  \`dxc\` could not load libdxil.so. Every D3D12 driver rejects an unsigned" >&2
        echo "  container, so this artifact would fail at pipeline creation and nowhere else." >&2
        exit 1
    fi
}

STATUS=0
SHADERS=(shaders/*.slang)
if [ ! -e "${SHADERS[0]}" ]; then
    echo "crcbl shaders: no shaders/*.slang to compile" >&2
    exit 1
fi

for SOURCE in "${SHADERS[@]}"; do
    NAME="$(basename "$SOURCE" .slang)"
    ARTIFACT="spirv/${NAME}.spv"
    FRESH="$WORK/${NAME}.spv"
    WGSL_ARTIFACT="wgsl/${NAME}.wgsl"
    FRESH_WGSL="$WORK/${NAME}.wgsl"
    MSL_ARTIFACT="msl/${NAME}.metal"
    FRESH_MSL="$WORK/${NAME}.metal"

    # `-fvk-use-entrypoint-name` keeps the *source* entry-point name in
    # `OpEntryPoint`. Without it Slang renames a module's only entry point to
    # `main`, while the WGSL and MSL targets keep the real name — so a
    # single-entry-point module ends up addressed as `main` on Vulkan and as
    # `computeMain` everywhere else, and this crate's cross-target name tests
    # fail. It is a no-op for every module with two entry points, which is why
    # the existing artifacts are byte-identical with and without it.
    echo "crcbl shaders: compiling $SOURCE → SPIR-V"
    "$SLANGC" "$SOURCE" \
        -target spirv \
        -profile "$SLANG_PROFILE" \
        -emit-spirv-directly \
        -fvk-use-entrypoint-name \
        -o "$FRESH"

    echo "crcbl shaders: compiling $SOURCE → WGSL"
    "$SLANGC" "$SOURCE" \
        -target wgsl \
        -profile "$SLANG_PROFILE" \
        -o "$FRESH_WGSL"

    # The Metal backend compiles this at run time through
    # `MTLDevice::newLibraryWithSource:options:error:`, so the artifact is MSL
    # *source* rather than a `.metallib` — there is no offline Metal compiler
    # off macOS, and committing one would put a macOS-only step in the middle of
    # a script every leg of CI runs.
    echo "crcbl shaders: compiling $SOURCE → MSL"
    "$SLANGC" "$SOURCE" \
        -target metal \
        -profile "$SLANG_PROFILE" \
        -o "$FRESH_MSL"

    # A compiler that emitted something the driver will reject is worse than one
    # that failed, because the failure moves to `vkCreateShaderModule` on
    # someone else's machine.
    if command -v spirv-val >/dev/null 2>&1; then
        spirv-val --target-env vulkan1.3 "$FRESH"
    else
        echo "crcbl shaders: no spirv-val; skipping validation of $ARTIFACT" >&2
    fi

    # `OpEntryPoint`'s execution model and name, straight out of the artifact
    # rather than restated by hand — the manifest must describe what was built.
    if command -v spirv-dis >/dev/null 2>&1; then
        # The entry-point *name* is matched against `OpEntryPoint` at pipeline
        # creation, so its case is load-bearing and must survive verbatim; only
        # the execution model is normalised to the seam's lower-case spelling.
        ENTRY_POINTS="$(spirv-dis "$FRESH" \
            | sed -n 's/.*OpEntryPoint \([A-Za-z]*\) %[^ ]* "\([^"]*\)".*/\2:\L\1/p' \
            | paste -sd, - \
            | sed 's/,/, /g')"
    else
        echo "crcbl shaders: no spirv-dis; cannot record entry points" >&2
        exit 1
    fi
    if [ -z "$ENTRY_POINTS" ]; then
        echo "crcbl shaders: $SOURCE declares no entry points" >&2
        exit 1
    fi

    # **DXIL is one container per entry point**, unlike the other three targets.
    # `dxc` compiles a single `-E`, and a graphics pipeline state object takes
    # one bytecode blob per stage, so there is nothing for a module carrying two
    # entry points to be. The manifest therefore records one `dxil` line per
    # entry point rather than one per shader.
    DXIL_LINES=()
    for PAIR in ${ENTRY_POINTS//,/ }; do
        ENTRY="${PAIR%%:*}"
        MODEL="${PAIR##*:}"
        PREFIX="$(dxc_profile_prefix "$MODEL")"
        if [ -z "$PREFIX" ]; then
            echo "crcbl shaders: $SOURCE entry point $ENTRY has execution model '$MODEL'," >&2
            echo "  which no DXIL graphics or compute profile covers." >&2
            exit 1
        fi
        DXIL_ARTIFACT="dxil/${NAME}.${ENTRY}.dxil"
        FRESH_HLSL="$WORK/${NAME}.${ENTRY}.hlsl"
        FRESH_DXIL="$WORK/${NAME}.${ENTRY}.dxil"

        echo "crcbl shaders: compiling $SOURCE:$ENTRY → HLSL → DXIL ($PREFIX"_"$DXIL_MODEL)"
        "$SLANGC" "$SOURCE" \
            -target hlsl \
            -entry "$ENTRY" \
            -stage "$(slang_stage "$MODEL")" \
            -o "$FRESH_HLSL"
        "$DXC" \
            -T "${PREFIX}_${DXIL_MODEL}" \
            -E "$ENTRY" \
            -Fo "$FRESH_DXIL" \
            "$FRESH_HLSL"
        require_signed_dxil "$FRESH_DXIL"

        if [ "$CHECK_ONLY" -eq 1 ]; then
            if ! cmp -s "$FRESH_DXIL" "$DXIL_ARTIFACT"; then
                echo "crcbl shaders: $DXIL_ARTIFACT does not match a fresh compile of $SOURCE." >&2
                echo "  Run crates/crcbl-shaders/tools/compile-shaders.sh and commit the result." >&2
                STATUS=1
            fi
        else
            cp "$FRESH_DXIL" "$DXIL_ARTIFACT"
        fi
        DXIL_LINES+=("dxil = ${ENTRY}:${DXIL_ARTIFACT}:$(sha256sum "$FRESH_DXIL" | cut -d' ' -f1)")
    done

    if [ "$CHECK_ONLY" -eq 1 ]; then
        if ! cmp -s "$FRESH" "$ARTIFACT"; then
            echo "crcbl shaders: $ARTIFACT does not match a fresh compile of $SOURCE." >&2
            echo "  Run crates/crcbl-shaders/tools/compile-shaders.sh and commit the result." >&2
            echo "  A rendering change that shifts output must also re-bless its golden" >&2
            echo "  image (docs/plan/12-testing.md)." >&2
            STATUS=1
        fi
        if ! cmp -s "$FRESH_WGSL" "$WGSL_ARTIFACT"; then
            echo "crcbl shaders: $WGSL_ARTIFACT does not match a fresh compile of $SOURCE." >&2
            echo "  Run crates/crcbl-shaders/tools/compile-shaders.sh and commit the result." >&2
            STATUS=1
        fi
        if ! cmp -s "$FRESH_MSL" "$MSL_ARTIFACT"; then
            echo "crcbl shaders: $MSL_ARTIFACT does not match a fresh compile of $SOURCE." >&2
            echo "  Run crates/crcbl-shaders/tools/compile-shaders.sh and commit the result." >&2
            STATUS=1
        fi
    else
        cp "$FRESH" "$ARTIFACT"
        cp "$FRESH_WGSL" "$WGSL_ARTIFACT"
        cp "$FRESH_MSL" "$MSL_ARTIFACT"
    fi

    {
        echo
        echo "[$NAME]"
        echo "source = $SOURCE"
        echo "source-sha256 = $(sha256sum "$SOURCE" | cut -d' ' -f1)"
        echo "spirv = $ARTIFACT"
        echo "spirv-sha256 = $(sha256sum "$FRESH" | cut -d' ' -f1)"
        echo "wgsl = $WGSL_ARTIFACT"
        echo "wgsl-sha256 = $(sha256sum "$FRESH_WGSL" | cut -d' ' -f1)"
        echo "msl = $MSL_ARTIFACT"
        echo "msl-sha256 = $(sha256sum "$FRESH_MSL" | cut -d' ' -f1)"
        printf '%s\n' "${DXIL_LINES[@]}"
        echo "entry-points = $ENTRY_POINTS"
    } >>"$MANIFEST"
done

if [ "$CHECK_ONLY" -eq 1 ]; then
    if ! diff -u spirv/manifest.txt "$MANIFEST" >/dev/null 2>&1; then
        echo "crcbl shaders: spirv/manifest.txt is stale:" >&2
        diff -u spirv/manifest.txt "$MANIFEST" >&2 || true
        STATUS=1
    fi
    if [ "$STATUS" -eq 0 ]; then
        echo "crcbl shaders: every artifact matches its source (slangc $SLANG_VERSION, dxc $DXC_VERSION)"
    fi
    exit "$STATUS"
fi

cp "$MANIFEST" spirv/manifest.txt
echo "crcbl shaders: regenerated ${#SHADERS[@]} artifact(s) and spirv/manifest.txt"
