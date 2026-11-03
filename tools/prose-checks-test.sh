#!/usr/bin/env bash
# Prose checks must distinguish a clean scan from a scan that never completed.
set -euo pipefail

script_dir=$(dirname "$0")
cd "$script_dir/.."

# The child Bash runs the checks, which invoke these exported command mocks.
# shellcheck disable=SC2317,SC2329
check() {
  local script=$1 mode=$2 expected=$3 result=0 output
  shift 3
  output=$(
    case "$mode" in
      clean) ;;
      missing-grep)
        grep() { return 127; }
        export -f grep
        ;;
      grep-error)
        grep() { return 2; }
        export -f grep
        ;;
      sed-error)
        sed() { return 2; }
        export -f sed
        ;;
      sort-error)
        sort() { return 2; }
        export -f sort
        ;;
      tr-error)
        tr() { return 2; }
        export -f tr
        ;;
      git-error)
        git() { printf '%s\n' rust-toolchain.toml; return 2; }
        export -f git
        ;;
      empty-git)
        git() { return 0; }
        export -f git
        ;;
      *) printf 'unknown probe: %s\n' "$mode" >&2; exit 1 ;;
    esac
    "$BASH" "$script" "$@" 2>&1
  ) || result=$?
  if [ "$result" -ne "$expected" ]; then
    printf '%s (%s): expected exit %s, got %s\n%s\n' \
      "$script" "$mode" "$expected" "$result" "$output" >&2
    exit 1
  fi
  printf '%s (%s): exit %s\n' "$script" "$mode" "$result"
}

missing=tools/nonexistent-prose-check-input.rs
if [ -e "$missing" ]; then
  printf 'the missing-input probe must not exist: %s\n' "$missing" >&2
  exit 1
fi

for script in tools/check-doc-citations.sh tools/check-wrapped-strings.sh; do
  check "$script" clean 0 rust-toolchain.toml
  check "$script" missing-grep 127 rust-toolchain.toml
  check "$script" grep-error 2 rust-toolchain.toml
  check "$script" git-error 2
  check "$script" empty-git 1
done

for mode in sed-error sort-error tr-error; do
  check tools/check-doc-citations.sh "$mode" 2 rust-toolchain.toml
done

check tools/check-doc-citations.sh clean 1 "$missing"
check tools/check-doc-citations.sh clean 1 tools
check tools/check-wrapped-strings.sh clean 2 "$missing"
check tools/check-wrapped-strings.sh clean 2 tools

# The wrapped-strings pattern itself: a joined literal is reported whatever the
# continuation resumes with, and the shapes it must leave alone are not — an
# aligned `label      {value}` column, and indentation after an escaped
# newline. The capital, `(` and `[` rows are the shapes the pattern missed
# until 2026-11-04.
probe=$(mktemp -t crcbl-wrapped-XXXXXX.rs)
trap 'rm -f "$probe"' EXIT
wrapped() {
  printf '%s\n' "$1" >"$probe"
  check tools/check-wrapped-strings.sh clean "$2" "$probe"
}
wrapped 'const A: &str = "the hint says press it,          and hold it";' 1
wrapped 'const B: &str = "the hint says press it,          Then hold it";' 1
wrapped 'const C: &str = "this backend refused the window          ({error}); next";' 1
wrapped 'const D: &str = "the menu opens here          [when asked] by the player";' 1
wrapped 'const E: &str = "{label}:          {value}";' 0
wrapped 'const F: &str = "a line of text\n          Indented on purpose";' 0
