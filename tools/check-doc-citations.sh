#!/usr/bin/env bash
# Every repository path a doc or a doc comment cites in backticks must exist.
#
#   tools/check-doc-citations.sh [file…]   # default: every tracked file that
#                                          # carries prose, bar the changelog
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
    # The editor is planned, not built. `docs/plan/08-editor.md` is its design.
    'apps/editor') return 0 ;;
  esac
  return 1
}

# Default: every tracked markdown file except the changelog. A changelog entry
# is a dated account of a release, and the path it named was right on the day —
# rewriting those to chase a later move would make the history wrong instead of
# stale. Everything else describes the tree as it is now, so it has to resolve.
# The directory of the nearest `Cargo.toml` at or above a file, or empty.
crate_root_of() {
  dir=$(dirname "$1")
  while [ "$dir" != "." ] && [ "$dir" != "/" ]; do
    if [ -e "$dir/Cargo.toml" ]; then
      printf '%s' "$dir"
      return 0
    fi
    dir=$(dirname "$dir")
  done
  return 0
}

files=("$@")
if [ ${#files[@]} -eq 0 ]; then
  mapfile -t files < <(
    git ls-files '*.md' '*.rs' '*.mjs' '*.js' '*.yml' '*.sh' '*.slang' |
      grep -v '^CHANGELOG.md$'
  )
fi

status=0
checked=0
for file in "${files[@]}"; do
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    checked=$((checked + 1))
    # Three ordinary ways to write a citation, so all three resolve: from the
    # repository root, from the doc's own directory (`web/README.md` says a
    # bare tools/serve.mjs), and from the owning crate's root — which is how
    # `crcbl-shaders`' sources say a bare tools/compile-shaders.sh for a script
    # beside their `Cargo.toml`.
    if [ -e "$path" ] ||
      [ -e "$(dirname "$file")/$path" ] ||
      { crate=$(crate_root_of "$file") && [ -n "$crate" ] && [ -e "$crate/$path" ]; } ||
      allowed "$path"; then
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
