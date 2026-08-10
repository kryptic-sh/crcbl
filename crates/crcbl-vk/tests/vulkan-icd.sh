#!/usr/bin/env bash
# Pinning a Vulkan ICD, for any harness that needs the driver CI runs.
#
# **Sourced, never run.** It exports into the caller's shell and exits it on
# failure, which is what the one caller did when it carried this inline:
#
#   source "…/crates/crcbl-vk/tests/vulkan-icd.sh"
#   crcbl_pin_vk_icd "crcbl vk e2e"      # honours $CRCBL_VK_ICD; a no-op unset
#
# `crcbl-vk` owns it because which manifest names a driver is its knowledge, but
# `crcbl`'s render suite sources it too: that suite draws a frame on whichever
# backend `CRCBL_GPU` names, and when the answer is `vk` it needs the same
# driver under the same name or it is not the run CI makes.
#
# # Why this is a function and not two exported variables in the workflow
#
# **Distributions disagree about the file name**: Debian ships
# `lvp_icd.x86_64.json` and Arch ships `lvp_icd.json`. A workflow step that
# writes one of them into `VK_DRIVER_FILES` works on the distribution the author
# had and fails everywhere else with `ERROR_INCOMPATIBLE_DRIVER` — the loader
# reports a missing manifest and a wrong manifest identically, so the failure
# names neither the file nor the mistake. That is exactly how it was found: a
# hardcoded Debian path in a new CI step, on a runner whose file was the other
# spelling.

# Resolve `$CRCBL_VK_ICD` and export both loader spellings.
#
# Unset means "let the loader choose", which is a legitimate way to run against
# real hardware — the caller says so, since only it knows what its own green run
# would then be worth. Set-but-unresolvable is a hard failure: a pinned ICD that
# quietly fell back to whatever was installed would defeat the pinning.
crcbl_pin_vk_icd() {
    local label="${1:-crcbl}"
    [ -n "${CRCBL_VK_ICD:-}" ] || return 0

    if [ ! -f "$CRCBL_VK_ICD" ]; then
        # Look next to the name that was asked for rather than encoding either
        # convention. `lvp_icd.x86_64.json` and `lvp_icd.json` share the stem
        # before the first dot of the *basename*; anything matching it in the
        # same directory is the same driver packaged differently.
        local icd_dir icd_stem found candidate
        icd_dir="$(dirname "$CRCBL_VK_ICD")"
        icd_stem="$(basename "$CRCBL_VK_ICD")"
        icd_stem="${icd_stem%%.*}"
        found=""
        for candidate in "${icd_dir}/${icd_stem}".json "${icd_dir}/${icd_stem}".*.json; do
            if [ -f "$candidate" ]; then
                found="$candidate"
                break
            fi
        done
        if [ -z "$found" ]; then
            echo "${label}: CRCBL_VK_ICD=$CRCBL_VK_ICD does not exist, and no sibling matched" >&2
            ls -la "$icd_dir" >&2 || true
            exit 1
        fi
        echo "${label}: $CRCBL_VK_ICD is absent; using $found"
        CRCBL_VK_ICD="$found"
    fi

    export CRCBL_VK_ICD

    # **The loader is a native Windows process and wants a native path.** Under
    # Git Bash the manifest arrives as `C:/lavapipe/...`, and the loader took
    # that as no pin at all: its first run said
    # `windows_read_data_files_in_registry: Registry lookup failed to get ICD
    # manifest files` and then `vkCreateInstance: Found no drivers!` — the
    # message it prints when the variable was never seen, not when it was seen
    # and rejected. Both spellings then answered `ERROR_INCOMPATIBLE_DRIVER`,
    # which is also what a genuinely incompatible manifest gives, so the failure
    # named neither the path nor the form.
    local icd_native="$CRCBL_VK_ICD"
    case "$(uname -s)" in
        MINGW* | MSYS* | CYGWIN*)
            if command -v cygpath >/dev/null 2>&1; then
                icd_native="$(cygpath -w "$CRCBL_VK_ICD")"
                echo "${label}: native ICD path $icd_native"
            fi
            ;;
    esac

    # Both spellings: `VK_DRIVER_FILES` is the current one and
    # `VK_ICD_FILENAMES` is what older loaders read.
    #
    # **An existing value wins.** Exporting these from Git Bash did not reach
    # the loader at all — the native path form was one cause and fixing it left
    # `windows_read_data_files_in_registry: Registry lookup failed` unchanged,
    # which is the loader saying it never saw the variable. So on Windows CI
    # sets them in the job environment, where the process that reads them is
    # already native, and this only fills them in when nobody else has. That is
    # also the right precedence on any platform: a caller who set the loader's
    # own variables meant it.
    export VK_DRIVER_FILES="${VK_DRIVER_FILES:-$icd_native}"
    export VK_ICD_FILENAMES="${VK_ICD_FILENAMES:-$icd_native}"
    echo "${label}: pinned ICD $CRCBL_VK_ICD"
}
