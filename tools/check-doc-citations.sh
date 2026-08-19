#!/usr/bin/env bash
# Every repository path a markdown doc cites in backticks must exist.
#
#   tools/check-doc-citations.sh [file…]      # default: docs/backlog.md
#
# `docs/backlog.md` is the working record, and its worth is that an entry can be
# acted on months later. A citation that no longer resolves defeats that
# silently: the reader greps, finds nothing, and concludes the work was deleted
# rather than moved. That has happened here — a test subtree moved from
# `crates/crcbl-vk/tests/vk_e2e/` into the `crcbl` crate's own suites, and the
# prose kept pointing at the old home long after.
#
# Nothing else catches it. Prettier reads the file as prose, rustdoc never sees
# it, and no test opens it. So this does, over the paths that are checkable
# without guessing: a backtick-quoted string beginning with one of the
# repository's top-level directories.
#
# Not checked: bare symbol names. `Foo::bar` in prose cannot be resolved without
# a compiler, and a gate that asks a question it cannot answer is worse than no
# gate. Paths are the half that is decidable.
set -euo pipefail

cd "$(dirname "$0")/.."

# Paths that are deliberately not in the tree, each with the reason it is cited.
# A path belongs here only when the doc is describing something that does not
# exist on purpose — a shape being proposed, or a stale citation in another file
# that the entry exists to report.
allowed() {
  case "$1" in
    # A layout the entry proposes if `web.rs` is ever split; not a real path.
    'crates/crcbl/src/web/') return 0 ;;
    # The shared launcher an entry argues for. `web/tools/browser-launch.mjs`
    # is the part of it that has since been built.
    'web/tools/chromium.mjs') return 0 ;;
    # Cited *because* it is stale: the entry reports that `docs/code-review.md`
    # still points at a file that no longer exists.
    'crates/crcbl-server/tests/integration.rs') return 0 ;;
  esac
  return 1
}

files=("$@")
if [ ${#files[@]} -eq 0 ]; then
  files=(docs/backlog.md)
fi

status=0
checked=0
for file in "${files[@]}"; do
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    checked=$((checked + 1))
    if [ -e "$path" ] || allowed "$path"; then
      continue
    fi
    # `grep -n` so the report is a place to go, not just a name.
    line=$(grep -n -F -- "\`$path\`" "$file" | head -1 | cut -d: -f1)
    printf '%s:%s: cites a path that does not exist: %s\n' "$file" "${line:-?}" "$path"
    status=1
  done < <(
    # The delimiter is a backtick; naming it keeps it out of a quoted pattern.
    tick='`'
    grep -oE "$tick(crates|apps|web|docs|tools|\.github)/[A-Za-z0-9_./+-]+$tick" "$file" |
      tr -d "$tick" | sed 's/[.,;:]$//' | sort -u
  )
done

if [ "$status" -eq 0 ]; then
  printf 'doc citations: %s path(s) checked, all resolve\n' "$checked"
else
  printf '\nEach line above is a path the docs name and the tree does not have.\n'
  printf 'Move the citation to where the file went, or add it to allowed() with\n'
  printf 'the reason it is deliberately absent.\n'
fi
exit "$status"
