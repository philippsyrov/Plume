#!/bin/bash
#
# Plume verification script.
#
# Designed to work even when no toolchain is installed yet. Stages:
#   1. Structure   — required files present (always run)
#   2. Guardrails  — no Electron in manifests, no duplicate agent file
#   3. Rust        — cargo fmt; clippy if PLUME_FULL_VERIFY=1
#   4. Frontend    — tsc --noEmit + Vitest tests
#                    (skipped without node + node_modules)
#   5. File sizes  — soft decomposition guardrail (warn-only; see
#                    docs/DECOMPOSITION.md)
#
# Exits 1 on any hard FAIL. WARNs do not fail the build.
# Run from anywhere; the script cd's to the project root.
#

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT" || exit 2

# Make rustup-installed Cargo discoverable even when the calling shell
# (a git hook, a fresh subshell, a GUI git client) didn't source
# ~/.cargo/env. Without this the Rust section silently WARNs "cargo not
# installed" on machines where rustup did install Cargo — which made
# the pre-commit hook a misleading source of local truth.
# No-op when cargo is already on PATH or rustup wasn't used.
if [ -d "$HOME/.cargo/bin" ]; then
  case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *) PATH="$HOME/.cargo/bin:$PATH" ;;
  esac
fi

FAIL=0
WARN=0
PASS=0

ok()      { printf "  [OK]   %s\n" "$1"; PASS=$((PASS + 1)); }
warn()    { printf "  [WARN] %s\n" "$1"; WARN=$((WARN + 1)); }
fail()    { printf "  [FAIL] %s\n" "$1"; FAIL=$((FAIL + 1)); }
section() { printf "\n== %s ==\n" "$1"; }

# ---- 1. Structure ----
section "Structure"

REQUIRED_FILES=(
  "AGENTS.md"
  "README.md"
  "docs/PLUME_PROJECT_SPEC.md"
  "docs/ARCHITECTURE.md"
  "docs/AGENT_OPERABILITY.md"
  "docs/MODEL_PROVIDERS.md"
  "docs/UI_STYLE.md"
  "docs/SAFETY.md"
  "docs/DEVELOPMENT.md"
  "docs/SMOKE_TESTING.md"
  "docs/DEPENDENCY_ISOLATION.md"
  "docs/IPC_CONTRACT.md"
  "docs/BOOTSTRAP.md"
  "docs/DECOMPOSITION.md"
  "scripts/dev-env.sh"
  "scripts/verify.sh"
  "scripts/check-file-sizes.sh"
)
for f in "${REQUIRED_FILES[@]}"; do
  if [ -f "$f" ]; then
    ok "$f present"
  else
    fail "$f missing"
  fi
done

# ---- 2. Guardrails ----
section "Guardrails"

# 2a. No Electron in dependency manifests.
ELECTRON_HIT=0
if [ -f "package.json" ] && grep -E '"electron"[[:space:]]*:' package.json >/dev/null 2>&1; then
  fail "package.json declares an electron dependency — Plume must not use Electron"
  ELECTRON_HIT=1
fi
if [ -f "src-tauri/Cargo.toml" ] && grep -E '^electron[[:space:]]*=' src-tauri/Cargo.toml >/dev/null 2>&1; then
  fail "src-tauri/Cargo.toml declares an electron crate — Plume must not use Electron"
  ELECTRON_HIT=1
fi
if [ "$ELECTRON_HIT" -eq 0 ]; then
  ok "No Electron dependency in manifests"
fi

# 2b. AGENTS.md / CLAUDE.md duplication.
if [ -f "CLAUDE.md" ] && [ -f "AGENTS.md" ]; then
  fail "Both AGENTS.md and CLAUDE.md exist — consolidate into AGENTS.md"
else
  ok "Single agent instruction file"
fi

# 2c. No committed secrets in obvious places.
if [ -f ".env" ]; then
  fail ".env is checked into the project root — move it out and add to .gitignore"
else
  ok "No .env in project root"
fi

# 2d. Shell helpers parse and are executable.
if bash -n scripts/dev-env.sh >/dev/null 2>&1; then
  ok "scripts/dev-env.sh parses"
else
  fail "scripts/dev-env.sh has a shell syntax error"
fi

if [ -x "scripts/dev-env.sh" ]; then
  ok "scripts/dev-env.sh executable"
else
  fail "scripts/dev-env.sh is not executable (run: chmod +x scripts/dev-env.sh)"
fi

# ---- 3. Rust / Tauri ----
section "Rust / Tauri"

if ! command -v cargo >/dev/null 2>&1; then
  warn "cargo not installed — skipping Rust checks (install via https://rustup.rs)"
elif [ ! -f "src-tauri/Cargo.toml" ]; then
  warn "src-tauri/Cargo.toml not present — skipping Rust checks"
else
  if (cd src-tauri && cargo fmt --check >/dev/null 2>&1); then
    ok "cargo fmt clean"
  else
    fail "cargo fmt would reformat files (run: cd src-tauri && cargo fmt)"
  fi

  if [ "${PLUME_FULL_VERIFY:-0}" = "1" ]; then
    if (cd src-tauri && cargo clippy --all-targets -- -D warnings >/dev/null 2>&1); then
      ok "cargo clippy clean"
    else
      fail "cargo clippy reports warnings (run: cd src-tauri && cargo clippy --all-targets)"
    fi
  else
    warn "Skipping cargo clippy (set PLUME_FULL_VERIFY=1 to enable)"
  fi
fi

# ---- 4. Frontend ----
section "Frontend"

if ! command -v node >/dev/null 2>&1; then
  warn "node not installed — skipping frontend checks (install Node 20+)"
elif [ ! -f "package.json" ]; then
  warn "package.json not present — skipping frontend checks"
elif [ ! -d "node_modules" ]; then
  warn "node_modules missing — run 'npm install' to enable TypeScript checks"
else
  if npx --no-install tsc --noEmit >/dev/null 2>&1; then
    ok "TypeScript type check clean"
  else
    fail "TypeScript type check failed (run: npm run typecheck)"
  fi

  if npm run test >/dev/null 2>&1; then
    ok "Frontend tests clean"
  else
    fail "Frontend tests failed (run: npm run test)"
  fi
fi

# ---- 5. File sizes (soft) ----
section "File sizes"

# Decomposition guardrail. Warn-only — see docs/DECOMPOSITION.md.
# The child script never exits non-zero in default mode, but its
# WARN lines feed into the WARN counter below via this wrapper so
# the summary reports them honestly.
if [ -x "scripts/check-file-sizes.sh" ]; then
  size_output="$(scripts/check-file-sizes.sh 2>&1)"
  size_exit=$?
  while IFS= read -r line; do
    case "$line" in
      *"[OK]"*)
        msg="${line#*\[OK\]   }"
        ok "$msg"
        ;;
      *"[WARN]"*)
        msg="${line#*\[WARN\] }"
        warn "$msg"
        ;;
      *)
        # Bare narrative lines (e.g. the trailing "---" summary
        # line) — print as-is without touching the counters.
        printf "%s\n" "$line"
        ;;
    esac
  done <<<"$size_output"
  if [ "$size_exit" -ne 0 ]; then
    # Child script in --strict mode (not how verify invokes it).
    # If it ever exits non-zero, surface that as a fail so a
    # manual strict run still blocks. Default CI path never sets
    # --strict.
    fail "scripts/check-file-sizes.sh reported exit $size_exit"
  fi
else
  warn "scripts/check-file-sizes.sh missing or not executable"
fi

# ---- Summary ----
section "Summary"
printf "  pass: %d   warn: %d   fail: %d\n" "$PASS" "$WARN" "$FAIL"

if [ "$FAIL" -gt 0 ]; then
  echo ""
  echo "Verification FAILED."
  exit 1
fi

echo ""
echo "Verification OK."
exit 0
