#!/usr/bin/env bash
# Fetches the viewer's model shelf: the Khronos CC0 models `apps/viewer` lists
# on its panel, at a pinned upstream commit, verified file by file.
#
#   ./tools/fetch-shelf.sh                 # fetch the whole shelf and verify it
#   ./tools/fetch-shelf.sh --check         # verify what is on disk; fetch nothing
#   ./tools/fetch-shelf.sh --web DIR       # fetch the browser subset into DIR too
#
# WHY THIS EXISTS RATHER THAN A DIRECTORY OF COMMITTED FILES. The whole shelf is
# about 138 MB. This repository uses no LFS and its history is not the place for
# a demo's texture set, so exactly one model is committed — Suzanne, which is
# what the viewer opens when nothing is asked for and what `apps/viewer`'s tests
# read with no network — and the rest arrives here. See `apps/viewer/src/shelf.rs`.
#
# THE FILE LIST IS NOT IN THIS SCRIPT. `apps/viewer/assets/shelf.sha256` is
# `sha256sum` format — a hash, two spaces, a path relative to the shelf root —
# and it is the one place the files of a model are written down; `shelf.rs`
# reads the same bytes through `include_str!`. A list in both would be a list
# that disagrees with itself the first time a model is re-exported upstream.
#
# EVERY FILE IS CHECKED AGAINST ITS HASH, and that is not paranoia about the
# network: it is what makes the pin mean something. `raw.githubusercontent.com`
# serves whatever the commit names, and a commit is immutable — so a file whose
# hash has moved is a file that is not the one the licences in `shelf.rs` were
# read against, whether it moved in transit or upstream.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The pinned upstream commit. `apps/viewer/src/shelf.rs`'s `UPSTREAM_COMMIT` is
# the same forty characters, and its
# `the_fetch_script_pins_the_commit_and_the_subset_this_table_names` reads this
# line to say so — a re-pin that moves only one of the two would fetch models
# nobody has read the licence of.
COMMIT=9429648735279342b4c32b8745f7904196607379
UPSTREAM="https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/$COMMIT/Models"

# The models the browser tab carries, in the order `shelf.rs` lists them. Read
# by the same test, against that table's `in_browser` rows: the demo site's
# asset budget is stated there, and a subset that disagreed would either put a
# row in the tab naming a document the site does not have, or ship megabytes
# nothing can open.
WEB_MODELS="Suzanne Avocado WaterBottle"

# The list, and where the files go. `CRCBL_SHELF` is the same variable the
# viewer reads at run time, so pointing both at one directory is how a build
# that is not run out of its source tree is served — and how a test gets a shelf
# of its own without touching this one.
LIST="$REPO/apps/viewer/assets/shelf.sha256"
SHELF="${CRCBL_SHELF:-$REPO/apps/viewer/assets/shelf}"

CHECK_ONLY=0
WEB_DIR=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --check)
      CHECK_ONLY=1
      shift
      ;;
    --web)
      if [ "$#" -lt 2 ]; then
        echo "crcbl fetch-shelf: --web needs a directory" >&2
        exit 2
      fi
      WEB_DIR="$2"
      shift 2
      ;;
    -h | --help)
      sed -n '2,8p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "crcbl fetch-shelf: unknown argument: $1" >&2
      echo "usage: ./tools/fetch-shelf.sh [--check] [--web DIR]" >&2
      exit 2
      ;;
  esac
done

if [ ! -f "$LIST" ]; then
  echo "crcbl fetch-shelf: $LIST is missing" >&2
  exit 1
fi

# macOS ships `shasum` and no `sha256sum`; Linux and the CI images ship both or
# the first. Named here once rather than branched at each call site.
if command -v sha256sum >/dev/null 2>&1; then
  sha256_of() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  echo "crcbl fetch-shelf: neither sha256sum nor shasum is on PATH" >&2
  exit 1
fi

# Whether this run cares about the model a shelf path belongs to.
#
# `--web` fetches the browser subset alone, which is the difference between a
# site build downloading 19 MB and downloading 138 MB it will not publish.
wanted() {
  local path="$1" model
  model="${path%%/*}"
  if [ -z "$WEB_DIR" ]; then
    return 0
  fi
  for web in $WEB_MODELS; do
    [ "$model" = "$web" ] && return 0
  done
  return 1
}

fetched=0
verified=0
failed=0

while read -r want path; do
  [ -n "$path" ] || continue
  wanted "$path" || continue

  target="$SHELF/$path"
  if [ -f "$target" ] && [ "$(sha256_of "$target")" = "$want" ]; then
    verified=$((verified + 1))
    continue
  fi

  if [ "$CHECK_ONLY" = "1" ]; then
    if [ -f "$target" ]; then
      echo "crcbl fetch-shelf: $path does not match its sha256" >&2
      echo "    wanted $want" >&2
      echo "    got    $(sha256_of "$target")" >&2
    else
      echo "crcbl fetch-shelf: $path is missing" >&2
    fi
    failed=$((failed + 1))
    continue
  fi

  echo "==> $path"
  mkdir -p "$(dirname "$target")"
  # To a scratch name and moved into place only once the hash matches, so an
  # interrupted run leaves nothing a later `--check` would call corrupt. `-f`
  # so an HTTP error is a non-zero exit rather than an error page written to
  # the file.
  if ! curl -sSfL --retry 3 --retry-delay 2 -o "$target.part" "$UPSTREAM/$path"; then
    echo "crcbl fetch-shelf: $path could not be downloaded" >&2
    rm -f "$target.part"
    failed=$((failed + 1))
    continue
  fi
  got="$(sha256_of "$target.part")"
  if [ "$got" != "$want" ]; then
    echo "crcbl fetch-shelf: $path does not match its sha256" >&2
    echo "    wanted $want" >&2
    echo "    got    $got" >&2
    rm -f "$target.part"
    failed=$((failed + 1))
    continue
  fi
  mv "$target.part" "$target"
  fetched=$((fetched + 1))
done < "$LIST"

if [ "$failed" != "0" ]; then
  echo "crcbl fetch-shelf: $failed file(s) are missing or do not match" >&2
  if [ "$CHECK_ONLY" = "1" ]; then
    echo "    ./tools/fetch-shelf.sh   # to fetch them" >&2
  fi
  exit 1
fi

if [ "$CHECK_ONLY" = "1" ]; then
  echo "crcbl fetch-shelf: $verified file(s) match $LIST"
  exit 0
fi
echo "crcbl fetch-shelf: $fetched fetched, $verified already correct, in $SHELF"

# The browser's copy. Into the caller's directory rather than into `web/`, so
# nothing untracked ever appears beside the demo's page: `web/build.sh` passes
# the site it has just assembled. `shelf.rs`'s `WEB_PREFIX` is the `shelf`
# below, and the demo's `assets/manifest.json` names the keys under it that the
# page pre-loads.
if [ -n "$WEB_DIR" ]; then
  echo "==> publishing the browser shelf to $WEB_DIR/shelf"
  # `glTF/.` into a directory that already exists, rather than the directory
  # itself: `cp -R src dst` copies *into* `dst` when `dst` is there, so the
  # obvious spelling nests a `glTF/glTF` on the second run. Idempotent this way,
  # and with no `rm` of a path built out of a variable.
  for web in $WEB_MODELS; do
    mkdir -p "$WEB_DIR/shelf/$web/glTF"
    cp -R "$SHELF/$web/glTF/." "$WEB_DIR/shelf/$web/glTF/"
  done
fi
