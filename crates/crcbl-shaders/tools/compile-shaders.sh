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
# # Every shader declares the targets it must support
#
# Each `.slang` carries one line naming the targets it is required to compile
# to:
#
#     // crcbl-targets: spirv, wgsl, msl, dxil
#
# and this script emits **exactly** those. It is a comment rather than a table
# somewhere else because a list that lives away from the shader is a list that
# drifts from it: a source moved, renamed or copied carries its own declaration
# with it, and a reviewer reading the first line of the file has already read
# the answer.
#
# The rule it enforces is `docs/plan/02-vulkan-backend.md`'s first shader
# portability rule — a required target that will not take the shader must
# **fail** here rather than emit a broken artifact. Two failure modes, both
# real:
#
# * A target Slang cannot express the shader in. The compile exits non-zero and
#   `set -e` stops the script, naming the source and the target.
# * A target a shader *stopped* declaring while its artifact stayed in the tree.
#   Nothing would regenerate that file and `build.rs` would go on hashing it, so
#   it would sit there getting older than its source and stay green. This script
#   refuses it by name and leaves the `git rm` to the author, because it has
#   never deleted anything and a generated-looking path is not proof.
#
# `spirv` is not optional. The entry-point list every other target's work is
# driven from — the DXIL profiles, the manifest's `entry-points` — is read out
# of the compiled SPIR-V with `spirv-dis`, so a shader with no SPIR-V is one
# nothing downstream can enumerate.
#
# Every shader shipped today declares all four. The mechanism exists ahead of
# the first shader that will not: a mesh-shader or ray-tracing source has no
# WGSL form at all, and must be able to say so before it is written rather than
# after its broken `.wgsl` is committed.
#
# The declaration is recorded in `spirv/manifest.txt` as a `targets` key, and
# `crcbl_shaders::manifest` refuses a record whose declaration and artifact
# columns disagree — so the check runs in `build.rs` too, on machines with no
# shader compiler at all.
#
# # Per-target defines
#
# **Slang defines no target macro of its own.** Probing 2026.14 turned up only
# `__SLANG_COMPILER__`, which is true for every target, so there was no way to
# write a `#if` that differs per target and the only way to differ at all was to
# fork the file — which is what `shaders/ui_tier_b.slang` was, until `ui.slang`
# stopped needing it. So each invocation below defines exactly one of:
#
#     CRCBL_TARGET_SPIRV  CRCBL_TARGET_WGSL  CRCBL_TARGET_MSL  CRCBL_TARGET_HLSL
#
# named after **the language coming out**, not after the declared target: `msl`
# is emitted by `-target metal` and `dxil` by `-target hlsl` and then `dxc`, and
# a shader that conditions on one is conditioning on that language's syntax and
# capabilities. The DXIL leg therefore defines `CRCBL_TARGET_HLSL`.
#
# `build.rs` passes the same define on the same invocation when it does its
# byte-for-byte recompile, and the two lists must stay in step: a define here
# that is missing there makes every artifact fail that check for a reason which
# is not drift.
#
# ENVIRONMENT
#   CRCBL_SLANGC   Path to `slangc`. Default: whatever is on PATH.
#   CRCBL_DXC      Path to `dxc`. **Required**, with no PATH fallback.

set -euo pipefail

# **The manifest's section order is a glob expansion, and a glob is sorted by
# the caller's locale.** `mesh.slang` and `mesh_shader.slang` order differently
# under `en_US.UTF-8` (which ignores the punctuation and puts `mesh_shader`
# first) than under `C` (where `.` is 0x2E and `_` is 0x5F, so `mesh.slang`
# comes first) — so two developers could commit byte-different manifests from
# identical sources, and one of them would fail `--check` on a machine that
# disagreed. That is exactly what happened on 2026-08-09: CI regenerated a
# correct manifest whose sections were in a different order and refused it.
#
# The compilers are pinned, so the environment has to be too.
export LC_ALL=C

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

# Every target a shader may declare, in the canonical order the manifest records
# them and this script emits them.
TARGETS=(spirv wgsl msl dxil)

# The same list spelled the way a declaration line and the manifest spell it, so
# every message quoting it is copy-pasteable into a shader.
ALL_TARGETS_LIST="${TARGETS[*]}"
ALL_TARGETS_LIST="${ALL_TARGETS_LIST// /, }"

# The `grep`/`sed` address of the declaration line. One expression, used by both
# the count and the extraction, so the two cannot come to disagree about what
# counts as a declaration.
TARGETS_DIRECTIVE='^//[[:space:]]*crcbl-targets:[[:space:]]*'

# Sets `DECLARED` to the targets `$1` declares, in `TARGETS` order.
#
# Exactly one declaration line is required. Zero means a shader that never
# stated what it must support, which is the state this rule exists to remove;
# two means the file answers the question twice and nothing says which answer
# the artifacts were built from.
DECLARED=""
read_declared_targets() {
    local source="$1" count raw target ordered=""
    count="$(grep -c "$TARGETS_DIRECTIVE" "$source" || true)"
    if [ "$count" != "1" ]; then
        echo "crcbl shaders: $source has $count \`// crcbl-targets:\` lines; it needs exactly 1." >&2
        echo "  Every shader declares the targets it must compile to, as its own first line:" >&2
        echo "    // crcbl-targets: ${ALL_TARGETS_LIST}" >&2
        echo "  naming any of: ${ALL_TARGETS_LIST}. \`spirv\` is required — the entry points" >&2
        echo "  every other target is driven from are read out of the compiled SPIR-V." >&2
        exit 1
    fi

    # `|` as the `s` delimiter, because the expression opens with `^//`.
    raw="$(sed -n "s|$TARGETS_DIRECTIVE||p" "$source" | tr ',' ' ')"
    for target in $raw; do
        case " ${TARGETS[*]} " in
        *" $target "*) ;;
        *)
            echo "crcbl shaders: $source declares target '$target', which is not one of:" >&2
            echo "  ${ALL_TARGETS_LIST}" >&2
            exit 1
            ;;
        esac
        case " $ordered " in
        *" $target "*)
            echo "crcbl shaders: $source declares target '$target' twice" >&2
            exit 1
            ;;
        esac
        ordered="$ordered $target"
    done

    # Re-emitted in `TARGETS` order rather than the author's, so the manifest
    # line is a function of the *set* and reordering a declaration is not drift.
    DECLARED=""
    for target in "${TARGETS[@]}"; do
        case " $ordered " in
        *" $target "*) DECLARED="$DECLARED $target" ;;
        esac
    done
    DECLARED="${DECLARED# }"

    if ! declares spirv; then
        echo "crcbl shaders: $source declares '${DECLARED// /, }', which does not include" >&2
        echo "  \`spirv\`. The entry-point list the manifest records — and the DXIL profiles" >&2
        echo "  every container is compiled at — are read out of the compiled SPIR-V with" >&2
        echo "  spirv-dis, so a shader without it is one nothing downstream can enumerate." >&2
        exit 1
    fi
}

# Whether the shader last passed to `read_declared_targets` declares target `$1`.
declares() {
    case " $DECLARED " in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
    esac
}

# Records an artifact that exists for a target its shader does not declare.
#
# This is the drift the declaration rule catches from the other side: a shader
# that *stopped* declaring a target leaves a file nothing regenerates, and
# `build.rs` keeps hashing it happily while it grows older than its source. The
# file is named rather than removed — this script has never deleted anything,
# and "it looks generated" is not proof that it is.
#
# `STATUS` rather than `exit`, so `--check` reports every stale artifact in one
# run instead of one per invocation. Reads `$SOURCE`, the shader the compile
# loop is on, for the same reason it writes `$STATUS`: both belong to that loop.
require_undeclared_artifact_absent() {
    local target="$1" artifact="$2"
    if [ -e "$artifact" ]; then
        echo "crcbl shaders: $SOURCE does not declare the '$target' target, but $artifact exists." >&2
        echo "  Either add '$target' to its \`// crcbl-targets:\` line, or \`git rm $artifact\`." >&2
        STATUS=1
    fi
}

# The DXIL profile prefix for one of `spirv-dis`' execution models, or nothing
# for a model no graphics or compute profile covers.
#
# `meshext` and `taskext` are how `spirv-dis` spells `VK_EXT_mesh_shader`'s two
# execution models; D3D12 calls the second one *amplification*, hence `as`.
dxc_profile_prefix() {
    case "$1" in
    vertex) echo "vs" ;;
    fragment) echo "ps" ;;
    glcompute | compute) echo "cs" ;;
    meshext) echo "ms" ;;
    taskext) echo "as" ;;
    *) echo "" ;;
    esac
}

# What `slangc -stage` calls one of `spirv-dis`' execution models.
slang_stage() {
    case "$1" in
    glcompute) echo "compute" ;;
    meshext) echo "mesh" ;;
    taskext) echo "amplification" ;;
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
    read_declared_targets "$SOURCE"
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
    #
    # `read_declared_targets` has already refused a shader that does not declare
    # `spirv`, so this leg is unconditional.
    echo "crcbl shaders: compiling $SOURCE → SPIR-V"
    "$SLANGC" "$SOURCE" \
        -target spirv \
        -profile "$SLANG_PROFILE" \
        -emit-spirv-directly \
        -fvk-use-entrypoint-name \
        -D CRCBL_TARGET_SPIRV=1 \
        -o "$FRESH"

    if declares wgsl; then
        echo "crcbl shaders: compiling $SOURCE → WGSL"
        "$SLANGC" "$SOURCE" \
            -target wgsl \
            -profile "$SLANG_PROFILE" \
            -D CRCBL_TARGET_WGSL=1 \
            -o "$FRESH_WGSL"
    else
        require_undeclared_artifact_absent wgsl "$WGSL_ARTIFACT"
    fi

    # The Metal backend compiles this at run time through
    # `MTLDevice::newLibraryWithSource:options:error:`, so the artifact is MSL
    # *source* rather than a `.metallib` — there is no offline Metal compiler
    # off macOS, and committing one would put a macOS-only step in the middle of
    # a script every leg of CI runs.
    if declares msl; then
        echo "crcbl shaders: compiling $SOURCE → MSL"
        "$SLANGC" "$SOURCE" \
            -target metal \
            -profile "$SLANG_PROFILE" \
            -D CRCBL_TARGET_MSL=1 \
            -o "$FRESH_MSL"
    else
        require_undeclared_artifact_absent msl "$MSL_ARTIFACT"
    fi

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

    # **A shader with an amplification stage also gets one SPIR-V module per
    # task and mesh entry point**, beside the combined one every stage is
    # otherwise drawn from.
    #
    # `slangc` gives a module holding a task entry point and its mesh partner
    # *two* `TaskPayloadWorkgroupEXT` variables — `__EmitMeshTasks_Payload`,
    # which the task writes, and one named for the mesh stage's `in payload`
    # parameter, which nothing writes. `spirv-val` accepts that, and the rule it
    # enforces is per entry point rather than per module, so it is legal SPIR-V.
    # An NVIDIA driver nonetheless hands the mesh stage the variable the task
    # never wrote, and the payload arrives as zeroes: every kept cluster then
    # draws cluster 0 of instance slot 0 of its bucket. Compiling one entry point
    # at a time leaves one payload variable per module, which is the shape every
    # driver is exercised on.
    #
    # Splitting only the mesh side does not work — with the task stage still
    # coming from the combined module the payload is still zeroes — so both
    # halves of the pair are emitted here.
    SPIRV_ENTRY_LINES=()
    HAS_TASK_STAGE=0
    for PAIR in ${ENTRY_POINTS//,/ }; do
        if [ "${PAIR##*:}" = "taskext" ]; then
            HAS_TASK_STAGE=1
        fi
    done
    for PAIR in ${ENTRY_POINTS//,/ }; do
        ENTRY="${PAIR%%:*}"
        MODEL="${PAIR##*:}"
        SPIRV_ENTRY_ARTIFACT="spirv/${NAME}.${ENTRY}.spv"
        if [ "$HAS_TASK_STAGE" -eq 0 ] || { [ "$MODEL" != "taskext" ] && [ "$MODEL" != "meshext" ]; }; then
            # Not a stage that needs one. Say so if a rename left the artifact
            # behind, rather than committing a module nothing reads.
            if [ -e "$SPIRV_ENTRY_ARTIFACT" ]; then
                echo "crcbl shaders: $SOURCE:$ENTRY needs no single-entry SPIR-V module," >&2
                echo "  but $SPIRV_ENTRY_ARTIFACT exists. \`git rm $SPIRV_ENTRY_ARTIFACT\`." >&2
                STATUS=1
            fi
            continue
        fi
        FRESH_SPIRV_ENTRY="$WORK/${NAME}.${ENTRY}.spv"

        echo "crcbl shaders: compiling $SOURCE:$ENTRY → SPIR-V"
        "$SLANGC" "$SOURCE" \
            -target spirv \
            -profile "$SLANG_PROFILE" \
            -emit-spirv-directly \
            -fvk-use-entrypoint-name \
            -D CRCBL_TARGET_SPIRV=1 \
            -entry "$ENTRY" \
            -o "$FRESH_SPIRV_ENTRY"

        if command -v spirv-val >/dev/null 2>&1; then
            spirv-val --target-env vulkan1.3 "$FRESH_SPIRV_ENTRY"
        fi

        if [ "$CHECK_ONLY" -eq 1 ]; then
            if ! cmp -s "$FRESH_SPIRV_ENTRY" "$SPIRV_ENTRY_ARTIFACT"; then
                echo "crcbl shaders: $SPIRV_ENTRY_ARTIFACT does not match a fresh compile of $SOURCE." >&2
                echo "  Run crates/crcbl-shaders/tools/compile-shaders.sh and commit the result." >&2
                STATUS=1
            fi
        else
            cp "$FRESH_SPIRV_ENTRY" "$SPIRV_ENTRY_ARTIFACT"
        fi
        SPIRV_ENTRY_LINES+=("spirv-entry = ${ENTRY}:${SPIRV_ENTRY_ARTIFACT}:$(sha256sum "$FRESH_SPIRV_ENTRY" | cut -d' ' -f1)")
    done

    # **DXIL is one container per entry point**, unlike the other three targets.
    # `dxc` compiles a single `-E`, and a graphics pipeline state object takes
    # one bytecode blob per stage, so there is nothing for a module carrying two
    # entry points to be. The manifest therefore records one `dxil` line per
    # entry point rather than one per shader.
    DXIL_LINES=()
    for PAIR in ${ENTRY_POINTS//,/ }; do
        ENTRY="${PAIR%%:*}"
        MODEL="${PAIR##*:}"
        DXIL_ARTIFACT="dxil/${NAME}.${ENTRY}.dxil"
        if ! declares dxil; then
            require_undeclared_artifact_absent dxil "$DXIL_ARTIFACT"
            continue
        fi
        PREFIX="$(dxc_profile_prefix "$MODEL")"
        if [ -z "$PREFIX" ]; then
            echo "crcbl shaders: $SOURCE entry point $ENTRY has execution model '$MODEL'," >&2
            echo "  which no DXIL graphics or compute profile covers." >&2
            exit 1
        fi
        FRESH_HLSL="$WORK/${NAME}.${ENTRY}.hlsl"
        FRESH_DXIL="$WORK/${NAME}.${ENTRY}.dxil"

        echo "crcbl shaders: compiling $SOURCE:$ENTRY → HLSL → DXIL ($PREFIX"_"$DXIL_MODEL)"
        "$SLANGC" "$SOURCE" \
            -target hlsl \
            -entry "$ENTRY" \
            -stage "$(slang_stage "$MODEL")" \
            -D CRCBL_TARGET_HLSL=1 \
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
        if declares wgsl && ! cmp -s "$FRESH_WGSL" "$WGSL_ARTIFACT"; then
            echo "crcbl shaders: $WGSL_ARTIFACT does not match a fresh compile of $SOURCE." >&2
            echo "  Run crates/crcbl-shaders/tools/compile-shaders.sh and commit the result." >&2
            STATUS=1
        fi
        if declares msl && ! cmp -s "$FRESH_MSL" "$MSL_ARTIFACT"; then
            echo "crcbl shaders: $MSL_ARTIFACT does not match a fresh compile of $SOURCE." >&2
            echo "  Run crates/crcbl-shaders/tools/compile-shaders.sh and commit the result." >&2
            STATUS=1
        fi
    else
        cp "$FRESH" "$ARTIFACT"
        if declares wgsl; then cp "$FRESH_WGSL" "$WGSL_ARTIFACT"; fi
        if declares msl; then cp "$FRESH_MSL" "$MSL_ARTIFACT"; fi
    fi

    # A target the shader does not declare contributes no artifact *and* no
    # manifest key, so `crcbl_shaders::manifest` sees the same absence
    # `build.rs` sees on disk — which is what lets it refuse a record whose
    # declaration and columns disagree.
    {
        echo
        echo "[$NAME]"
        echo "source = $SOURCE"
        echo "source-sha256 = $(sha256sum "$SOURCE" | cut -d' ' -f1)"
        echo "targets = ${DECLARED// /, }"
        echo "spirv = $ARTIFACT"
        echo "spirv-sha256 = $(sha256sum "$FRESH" | cut -d' ' -f1)"
        if [ "${#SPIRV_ENTRY_LINES[@]}" -gt 0 ]; then
            printf '%s\n' "${SPIRV_ENTRY_LINES[@]}"
        fi
        if declares wgsl; then
            echo "wgsl = $WGSL_ARTIFACT"
            echo "wgsl-sha256 = $(sha256sum "$FRESH_WGSL" | cut -d' ' -f1)"
        fi
        if declares msl; then
            echo "msl = $MSL_ARTIFACT"
            echo "msl-sha256 = $(sha256sum "$FRESH_MSL" | cut -d' ' -f1)"
        fi
        if [ "${#DXIL_LINES[@]}" -gt 0 ]; then
            printf '%s\n' "${DXIL_LINES[@]}"
        fi
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

# A regenerate run has its own way to fail: a stale artifact for a target that
# is no longer declared. Writing the manifest anyway would record the tree as
# consistent while that file sits beside it, so the manifest is the thing held
# back — everything already copied is a faithful compile of its source.
if [ "$STATUS" -ne 0 ]; then
    echo "crcbl shaders: spirv/manifest.txt not written — resolve the above first" >&2
    exit "$STATUS"
fi

cp "$MANIFEST" spirv/manifest.txt
echo "crcbl shaders: regenerated ${#SHADERS[@]} artifact(s) and spirv/manifest.txt"
