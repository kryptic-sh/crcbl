#!/usr/bin/env bash
# A Rust string literal wrapped across lines keeps the newline and the next
# line's indentation unless the first line ends with a `\`. Forget it and the
# message a reader meets is `… so they are work that is              actually
# owed …` — a sentence with a hole in it.
#
#   tools/check-wrapped-strings.sh [file…]   # default: every tracked *.rs
#
# Nothing else catches this. It compiles, `rustfmt` leaves it alone, clippy has
# no lint for it, and the text is only ever seen when the assertion it belongs
# to fires — which is the moment someone is already reading it for a reason.
# Three of these were in the tree at once before anyone looked.
#
# The signal is a run of six or more spaces inside a string literal, following
# a letter or a piece of sentence punctuation. Indentation deliberately written
# into a literal is nearly always preceded by an escaped newline, so a space run
# whose preceding character is escaped is not reported: that is how the embedded
# shader source in `crcbl-shaders` and the fixture text in `crcbl-sprite` and
# `crcbl-wl-scanner` stay quiet without an exception list to maintain.
set -euo pipefail

cd "$(dirname "$0")/.."

files=("$@")
if [ ${#files[@]} -eq 0 ]; then
  mapfile -t files < <(git ls-files '*.rs')
fi

# A gate whose scope matches nothing reports success forever.
if [ ${#files[@]} -eq 0 ]; then
  printf 'wrapped strings: no files to check — the file list is empty\n' >&2
  exit 1
fi

# `(?<!\\)` is what excludes a literal `\n` followed by indentation: the `n` is
# a letter, and the run after it is the line the author asked for.
pattern='"[^"]*(?<!\\)[a-z,;.] {6,}[a-z][^"]*"'

if grep -nP "$pattern" "${files[@]}"; then
  printf '\nEach line above is a string literal wrapped without the trailing\n'
  printf 'backslash that joins its halves, so it prints with the newline and\n'
  printf "the next line's indentation inside the sentence. Add the backslash.\n"
  exit 1
fi

printf 'wrapped strings: %s file(s) checked, no collapsed literals\n' "${#files[@]}"
