#!/bin/bash
#
# File-size guardrail check for Plume.
#
# Emits warnings (not failures) when source files exceed the
# thresholds documented in docs/DECOMPOSITION.md:
#
#   code (*.ts, *.tsx, *.rs):
#     <= 400      green   no output
#     401-800     yellow  no output (acceptable, not warned)
#     801-1200    amber   WARN
#     > 1200      red     WARN
#
#   docs (*.md):
#     > 1500      WARN
#
# Tests living in their own files are exempt — anything matching
# *_test.rs, *_tests.rs, *.test.ts, *.test.tsx, tests/, or
# __tests__/ is skipped.
#
# Exit code is always 0 in default mode (warn-only). The script
# is wired into scripts/verify.sh but does not fail the build.
# See docs/DECOMPOSITION.md § "Future enforcement (later)" for
# when this hardens.
#
# Usage:
#   scripts/check-file-sizes.sh           # warn-only (default)
#   scripts/check-file-sizes.sh --strict  # exit 1 on any amber/red
#                                         # (manual use; CI does not
#                                         # pass --strict yet)
#
# Portable to macOS /bin/bash 3.2 — no mapfile, no arrays of paths.

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT" || exit 2

STRICT=0
if [ "${1:-}" = "--strict" ]; then
  STRICT=1
fi

CODE_AMBER=801
CODE_RED=1201
DOC_WARN=1501

amber_hits=0
red_hits=0
doc_hits=0

# Process substitution keeps the counter increments in the parent
# shell. A piped `find | while` would run the loop body in a
# subshell and lose the counts.

while IFS= read -r f; do
  [ -f "$f" ] || continue
  lines=$(wc -l <"$f" | tr -d ' ')
  if [ "$lines" -ge "$CODE_RED" ]; then
    printf "  [WARN] %s — %d lines (red, > %d). See docs/DECOMPOSITION.md refactor map.\n" \
      "$f" "$lines" $((CODE_RED - 1))
    red_hits=$((red_hits + 1))
  elif [ "$lines" -ge "$CODE_AMBER" ]; then
    printf "  [WARN] %s — %d lines (amber, > %d). Plan a split.\n" \
      "$f" "$lines" $((CODE_AMBER - 1))
    amber_hits=$((amber_hits + 1))
  fi
done < <(
  find src src-tauri -type f \
    \( -name "*.ts" -o -name "*.tsx" -o -name "*.rs" \) \
    ! -path "*/node_modules/*" \
    ! -path "*/target/*" \
    ! -path "*/dist/*" \
    ! -path "*/.claude/*" \
    ! -name "*_test.rs" \
    ! -name "*_tests.rs" \
    ! -name "*.test.ts" \
    ! -name "*.test.tsx" \
    ! -path "*/tests/*" \
    ! -path "*/__tests__/*" \
    2>/dev/null | sort
)

while IFS= read -r f; do
  [ -f "$f" ] || continue
  lines=$(wc -l <"$f" | tr -d ' ')
  if [ "$lines" -ge "$DOC_WARN" ]; then
    printf "  [WARN] %s — %d lines (doc soft cap %d). Consider narrowing.\n" \
      "$f" "$lines" $((DOC_WARN - 1))
    doc_hits=$((doc_hits + 1))
  fi
done < <(
  find docs -type f -name "*.md" \
    ! -path "*/.claude/*" \
    2>/dev/null | sort
)

total=$((amber_hits + red_hits + doc_hits))
if [ "$total" -eq 0 ]; then
  printf "  [OK]   No files past thresholds (code amber=%d red=%d, doc=%d).\n" \
    $((CODE_AMBER - 1)) $((CODE_RED - 1)) $((DOC_WARN - 1))
else
  printf "  ---    %d amber, %d red, %d doc soft-cap. Total: %d.\n" \
    "$amber_hits" "$red_hits" "$doc_hits" "$total"
fi

if [ "$STRICT" -eq 1 ] && [ "$total" -gt 0 ]; then
  exit 1
fi
exit 0
