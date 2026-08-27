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
# Markdown files get a second pass: the target of every relative link. The plan
# documents under `docs/plan/` are indexes of each other, so a renamed or deleted
# topic leaves dead links that the backtick pass cannot see — its patterns want a
# top-level directory, and a sibling link is a bare `44-lighting.md`. A link
# target resolves against its own file's directory, which is what a Markdown
# renderer does, so that is the only place this looks.
#
# Not checked: bare symbol names. `Foo::bar` in prose cannot be resolved without
# a compiler, and a gate that asks a question it cannot answer is worse than no
# gate. Paths are the half that is decidable. Rust doc comments write intra-doc
# links in the same `[text](Target)` shape, which is why the link pass is for
# Markdown files only and why a target has to look like a path — hold a `/` or an
# extension — before it is asked to exist.
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

  case "$file" in
    *.md) ;;
    *) continue ;;
  esac

  while IFS= read -r target; do
    [ -n "$target" ] || continue
    # A scheme or a bare fragment is not a repository path.
    case "$target" in
      '#'* | *://* | mailto:*) continue ;;
    esac
    # A renderer drops the fragment before it opens the file, so do the same.
    path=${target%%#*}
    [ -n "$path" ] || continue
    # Path-shaped or not asked about: this is what keeps a doc comment's
    # `[Foo](Self::bar)` out of a gate that has no compiler to resolve it.
    case "$path" in
      */*) ;;
      *.[A-Za-z0-9]*) ;;
      *) continue ;;
    esac
    checked=$((checked + 1))
    if [ -e "$(dirname "$file")/$path" ] || allowed "$path"; then
      continue
    fi
    line=$(grep -n -F -- "]($target)" "$file" | head -1 | cut -d: -f1)
    printf '%s:%s: links to a path that does not exist: %s\n' "$file" "${line:-?}" "$target"
    status=1
  done < <(
    # Inline code spans first: a doc that quotes a broken link as its example —
    # `docs/backlog.md` does, in the entry this pass closes — is showing the
    # syntax, not writing a link, and a gate that cannot tell them apart fails on
    # the file describing it.
    tick='`'
    sed "s/${tick}${tick}[^${tick}]*${tick}${tick}//g; s/${tick}[^${tick}]*${tick}//g" \
      "$file" |
      grep -oE '\]\([^)]+\)' | sed 's/^](//; s/)$//' | sort -u
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
