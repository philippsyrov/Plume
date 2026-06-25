#!/usr/bin/env bash
# D90: scripted, UI-free end-to-end smoke for the local-first happy path
# — a real Qwen model served by Plume-managed MLX. No computer-use, no
# manual UI driving, no Ollama, no downloads.
#
# This is the "does the local model actually answer?" proof. It:
#
#   1. resolves the Python interpreter the SAME way Plume's MLX
#      supervisor does — `PLUME_MLX_PYTHON` first (D58), then the
#      conventional venv at ~/.venvs/mlx-env, then python3 / python;
#   2. auto-discovers a Qwen checkpoint under the Plume model dir
#      (`$PLUME_MODEL_DIR`, else `<repo>/plume-models`), preferring a
#      Qwen2.5-Coder 3B 4-bit folder (the documented local target) but
#      accepting any classifiable Qwen folder;
#   3. delegates the actual runtime round-trip to
#      `scripts/smoke-mlx-runtime.sh`, which mirrors the supervisor's
#      path exactly: allocate an ephemeral port, spawn
#      `python -m mlx_lm server …`, poll `/health`, POST one tiny
#      `/v1/chat/completions`, validate the OpenAI-shaped reply, then
#      shut the child down;
#   4. prints a single clear PASS / FAIL banner with diagnostics.
#
# Why wrap `smoke-mlx-runtime.sh` rather than re-spawn here: that script
# already replicates the supervisor's spawn/health/chat/shutdown
# sequence and is the closest UI-free approximation of the real runtime
# path. D90 adds the Plume-specific discovery (which python, which Qwen
# folder) on top so an operator can run ONE command with no arguments.
#
# Exit status: 0 = PASS (a Qwen reply came back), non-zero = FAIL with a
# diagnostic explaining exactly which precondition was missing (no
# interpreter, mlx_lm not importable, no Qwen model, server never became
# healthy, …). It never installs packages, never downloads a model, and
# never modifies the model folder.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_SMOKE="$REPO_ROOT/scripts/smoke-mlx-runtime.sh"

PROMPT_TEXT="${PROMPT_TEXT:-Reply with the single word: pong}"
STARTUP_TIMEOUT="${STARTUP_TIMEOUT:-90}"
CHAT_TIMEOUT="${CHAT_TIMEOUT:-60}"

fail() {
  echo ""
  echo "================ SMOKE: FAIL ================"
  echo "$1" >&2
  shift || true
  for line in "$@"; do
    echo "$line" >&2
  done
  echo "============================================="
  exit 1
}

echo "=== D90 Qwen MLX chat smoke (UI-free) ==="
echo ""

# ─── Step 1: resolve the Python interpreter (Plume supervisor order) ─────

# Mirror the supervisor's resolution: PLUME_MLX_PYTHON wins (D58), then
# the conventional venv, then a plain interpreter. We only ACCEPT a
# candidate that can `import mlx_lm`, so a stray python3 without mlx-lm
# doesn't shadow a real venv later in the list.
candidate_pythons=()
[[ -n "${PLUME_MLX_PYTHON:-}" ]] && candidate_pythons+=("$PLUME_MLX_PYTHON")
candidate_pythons+=("$HOME/.venvs/mlx-env/bin/python")
candidate_pythons+=("python3" "python")

PYTHON_BIN=""
for cand in "${candidate_pythons[@]}"; do
  if command -v "$cand" >/dev/null 2>&1 && "$cand" -c "import mlx_lm" >/dev/null 2>&1; then
    PYTHON_BIN="$cand"
    break
  fi
done

if [[ -z "$PYTHON_BIN" ]]; then
  fail \
    "No Python with an importable 'mlx_lm' was found." \
    "" \
    "Tried (in order): ${candidate_pythons[*]}" \
    "" \
    "Set PLUME_MLX_PYTHON to the interpreter of your mlx-lm venv, e.g.:" \
    "  export PLUME_MLX_PYTHON=\$HOME/.venvs/mlx-env/bin/python" \
    "" \
    "To create that venv (no global install):" \
    "  python3 -m venv ~/.venvs/mlx-env" \
    "  ~/.venvs/mlx-env/bin/pip install --upgrade pip mlx-lm" \
    "" \
    "Note: mlx-lm requires Apple Silicon — this smoke only PASSes on a Mac."
fi

echo "[OK]   interpreter: $PYTHON_BIN"
echo "       mlx_lm: $("$PYTHON_BIN" -c 'import mlx_lm; print(mlx_lm.__version__)' 2>/dev/null || echo '?')"

# ─── Step 2: locate the Plume model dir + a Qwen checkpoint ──────────────

MODEL_DIR="${PLUME_MODEL_DIR:-$REPO_ROOT/plume-models}"
if [[ ! -d "$MODEL_DIR" ]]; then
  fail \
    "Plume model dir not found: $MODEL_DIR" \
    "" \
    "Set PLUME_MODEL_DIR or place a Qwen folder under <repo>/plume-models." \
    "This smoke never downloads — bring your own local model (Plume's" \
    "importer / Local models panel is the acquisition story, not this script)."
fi

# A folder is a usable checkpoint when it has config.json + a tokenizer
# + at least one weight file — the same floor Plume's scanner and
# smoke-mlx-runtime.sh enforce. Among Qwen folders, prefer a
# Coder-3B-4bit, then any Qwen.
is_checkpoint() {
  local dir="$1"
  [[ -f "$dir/config.json" ]] || return 1
  [[ -f "$dir/tokenizer.json" || -f "$dir/tokenizer.model" || -f "$dir/tokenizer_config.json" ]] || return 1
  find "$dir" -maxdepth 1 -type f \( -name '*.safetensors' -o -name '*.gguf' -o -name '*.npz' \) \
    | grep -q . || return 1
  return 0
}

# Collect candidate Qwen folders (case-insensitive name match), one per
# line, sorted so the preference scan below is deterministic. A
# `while read` loop instead of `mapfile` — stock macOS /bin/bash is 3.2,
# which has no `mapfile`/`readarray`.
qwen_dirs=()
while IFS= read -r qd; do
  [[ -n "$qd" ]] && qwen_dirs+=("$qd")
done < <(find "$MODEL_DIR" -maxdepth 2 -type d -iname '*qwen*' 2>/dev/null | sort)

MODEL_FOLDER=""
# First pass: a Coder 3B 4-bit checkpoint (the documented local target).
for d in "${qwen_dirs[@]}"; do
  base="$(basename "$d")"
  if [[ "$base" =~ [Cc]oder && "$base" =~ 3[Bb] && "$base" =~ 4 ]] && is_checkpoint "$d"; then
    MODEL_FOLDER="$d"
    break
  fi
done
# Second pass: any classifiable Qwen checkpoint.
if [[ -z "$MODEL_FOLDER" ]]; then
  for d in "${qwen_dirs[@]}"; do
    if is_checkpoint "$d"; then
      MODEL_FOLDER="$d"
      break
    fi
  done
fi

if [[ -z "$MODEL_FOLDER" ]]; then
  listing="$(find "$MODEL_DIR" -maxdepth 2 -type d 2>/dev/null | sed "s|^|  |" | head -n 20)"
  fail \
    "No classifiable Qwen checkpoint found under: $MODEL_DIR" \
    "" \
    "A checkpoint needs config.json + a tokenizer + a weight file" \
    "(.safetensors / .gguf / .npz) and a folder name containing 'qwen'." \
    "" \
    "Folders seen (up to 20):" \
    "$listing"
fi

echo "[OK]   Qwen checkpoint: $MODEL_FOLDER"
echo ""

# ─── Step 3: delegate to the runtime smoke (the supervisor's path) ───────

echo "[..]   handing off to smoke-mlx-runtime.sh (spawn → /health → chat)…"
echo ""

if PYTHON_BIN="$PYTHON_BIN" STARTUP_TIMEOUT="$STARTUP_TIMEOUT" CHAT_TIMEOUT="$CHAT_TIMEOUT" \
  PROMPT_TEXT="$PROMPT_TEXT" bash "$RUNTIME_SMOKE" "$MODEL_FOLDER"; then
  echo ""
  echo "================ SMOKE: PASS ================"
  echo "  interpreter: $PYTHON_BIN"
  echo "  model:       $MODEL_FOLDER"
  echo "  prompt:      $PROMPT_TEXT"
  echo "  The local Qwen model answered through the MLX runtime path."
  echo "============================================="
  exit 0
else
  fail \
    "The MLX runtime round-trip failed (see smoke-mlx-runtime.sh output above)." \
    "" \
    "interpreter: $PYTHON_BIN" \
    "model:       $MODEL_FOLDER"
fi
