#!/usr/bin/env bash
# CI's Hyprland: the Wayland suite against Hyprland on a runner with no GPU.
#
#   docker run --privileged -v /dev/dri:/dev/dri … archlinux:… \
#       crates/crcbl-shell/tests/hyprland-ci.sh [extra nextest args…]
#
# **Run as root inside an Arch container**, which `ci.yml`'s `hyprland-e2e`
# job starts after loading `vkms` on the runner. Arch, because Hyprland moves
# faster than any Ubuntu release packages it and the suite's Lua config needs a
# current one; the packages come from the Arch Linux Archive at
# `HYPRLAND_ARCHIVE_DATE`, so a run installs the same versions until that date
# is moved on purpose.
#
# Hyprland has no headless start, so it runs on vkms, the kernel's virtual
# display, and renders with Mesa's software rasteriser: `LIBGL_ALWAYS_SOFTWARE`
# is what lets its EGL initialise on a device with no render node. A seat comes
# from `seatd`. Then it drops to the runner's own user, whose toolchain and
# cargo caches are mounted at their runner paths, and runs
# `run-wayland-e2e-hyprland.sh` with `CRCBL_E2E_HYPRLAND_STANDALONE=1`.

set -euo pipefail

# The Arch Linux Archive snapshot the packages are installed from.
HYPRLAND_ARCHIVE_DATE="2026/11/27"

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

: "${CI_USER_UID:?the uid that owns the mounted workspace}"
: "${CI_USER_HOME:?the runner home the toolchain is mounted in}"

echo "Server = https://archive.archlinux.org/repos/${HYPRLAND_ARCHIVE_DATE}/\$repo/os/\$arch" \
    >/etc/pacman.d/mirrorlist
# `-Syyuu` moves every package the image already has to the snapshot too, up
# or down, so the image's own date does not leak into the run.
# The Archive is slow, not down: pacman's own stall timeout gave up on it
# mid-file, so it is off, and the job's timeout bounds the wait instead.
pacman -Syyuu --noconfirm --needed --disable-download-timeout \
    hyprland seatd mesa python gcc >/tmp/pacman.log 2>&1 || {
    tail -n 40 /tmp/pacman.log >&2
    echo "crcbl e2e: installing Hyprland from the ${HYPRLAND_ARCHIVE_DATE} archive failed" >&2
    exit 1
}
pacman -Q hyprland aquamarine mesa

# The vkms card, which the 7.0 kernels list under the faux bus.
DRM_DEVICE=""
for card in /sys/class/drm/card[0-9]*; do
    if grep -qE '^DRIVER=(vkms|faux_driver)$' "${card}/device/uevent" 2>/dev/null; then
        DRM_DEVICE="/dev/dri/${card##*/}"
    fi
done
if [ -z "$DRM_DEVICE" ]; then
    echo "crcbl e2e: no vkms card in /dev/dri; was the module loaded on the runner?" >&2
    exit 1
fi
echo "crcbl e2e: Hyprland gets ${DRM_DEVICE}"

# Mesa's software path opens the card itself, beside the seat's fd, so the user
# joins the group that owns it on the runner, which the image has no name for.
DRM_GID="$(stat -c %g "$DRM_DEVICE")"
getent group "$DRM_GID" >/dev/null || groupadd --gid "$DRM_GID" hostdrm
useradd --no-create-home --home-dir "$CI_USER_HOME" --uid "$CI_USER_UID" \
    --groups "$DRM_GID" runner
# Docker made the home as root to mount the toolchain inside it; Hyprland
# writes its crash reports under the user's cache.
chown runner "$CI_USER_HOME"
install -d -o runner "${CI_USER_HOME}/.cache"
RUNTIME_DIR=/run/user/${CI_USER_UID}
install -d -m 700 -o runner "$RUNTIME_DIR"

seatd -u runner -l error &
SEATD_PID=$!
trap 'kill "$SEATD_PID" 2>/dev/null || true' EXIT

su runner -s /bin/bash -c "$(printf '%q ' env \
    HOME="$CI_USER_HOME" \
    PATH="${CI_USER_HOME}/.cargo/bin:/usr/bin" \
    XDG_RUNTIME_DIR="$RUNTIME_DIR" \
    LIBSEAT_BACKEND=seatd \
    AQ_DRM_DEVICES="$DRM_DEVICE" \
    LIBGL_ALWAYS_SOFTWARE=1 \
    CRCBL_E2E_HYPRLAND_STANDALONE=1 \
    CARGO_TERM_COLOR=always \
    RUSTFLAGS="${RUSTFLAGS:-}" \
    "${CRATE_DIR}/tests/run-wayland-e2e-hyprland.sh" "$@")"
