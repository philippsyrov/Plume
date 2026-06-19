#!/usr/bin/env bash
# D91: Qwen propose-diff model-quality smoke. Answers the real question —
# can the local 3B/4-bit model produce a unified diff that survives
# Plume's own validate → apply → revert path? UI-free, no Ollama, no
# downloads, and no writes to real source files (only a throwaway temp
# fixture + Plume's own pre-apply checkpoint inside it).
#
# Flow:
#   1. resolve the interpreter (PLUME_MLX_PYTHON → ~/.venvs/mlx-env →
#      python3/python, mlx_lm-importable only) and a Qwen checkpoint
#      (same discovery as scripts/smoke-qwen-mlx.sh);
#   2. seed a temp fixture with greet.py;
#   3. start mlx-lm, ask the model to emit ONLY a unified diff editing
#      greet.py, capture the reply, strip any code fence;
#   4. hand the diff + fixture to Plume's real patch path via the
#      `#[ignore]`d Rust smoke test (validate → apply → revert);
#   5. print PASS / FAIL. An invalid or non-applying model diff is a
#      MODEL-QUALITY FAIL reported clearly — the machine state stays
#      clean because apply only runs after validate and is all-or-nothing
#      with rollback.
#
# This is model-quality smoke, not a guarantee: a small local model may
# well produce a malformed or non-applying diff. That's a real signal,
# not a script bug.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PROMPT_INSTRUCTION="${PROMPT_INSTRUCTION:-Rewrite greet so it returns an f-string: f\"Hello, {name}!\". Reply with ONLY a unified diff (lines starting with ---, +++, @@, space, - and +). No prose, no explanation, no code fence.}"
STARTUP_TIMEOUT="${STARTUP_TIMEOUT:-90}"
MAX_TOKENS="${MAX_TOKENS:-256}"

WORKDIR=""
SERVER_PID=""
LOG_FILE=""

cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    kill -INT -- "-$SERVER_PID" >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do
      kill -0 "$SERVER_PID" >/dev/null 2>&1 || break
      sleep 0.1
    done
    kill -KILL -- "-$SERVER_PID" >/dev/null 2>&1 || true
  fi
  [[ -n "$LOG_FILE" ]] && rm -f "$LOG_FILE"
  [[ -n "$WORKDIR" ]] && rm -rf "$WORKDIR"
}
trap cleanup EXIT

fail() {
  echo ""
  echo "============== PROPOSE-DIFF: FAIL =============="
  echo "$1" >&2
  shift || true
  for line in "$@"; do echo "$line" >&2; done
  echo "==============================================="
  exit 1
}

echo "=== D91 Qwen propose-diff smoke (UI-free) ==="
echo ""

# ─── Step 1: interpreter + Qwen checkpoint (shared discovery) ────────────

candidate_pythons=()
[[ -n "${PLUME_MLX_PYTHON:-}" ]] && candidate_pythons+=("$PLUME_MLX_PYTHON")
candidate_pythons+=("$HOME/.venvs/mlx-env/bin/python" "python3" "python")
PYTHON_BIN=""
for cand in "${candidate_pythons[@]}"; do
  if command -v "$cand" >/dev/null 2>&1 && "$cand" -c "import mlx_lm" >/dev/null 2>&1; then
    PYTHON_BIN="$cand"; break
  fi
done
[[ -z "$PYTHON_BIN" ]] && fail \
  "No Python with an importable 'mlx_lm' was found." \
  "Tried: ${candidate_pythons[*]}" \
  "Set PLUME_MLX_PYTHON=\$HOME/.venvs/mlx-env/bin/python (Apple Silicon only)."
echo "[OK]   interpreter: $PYTHON_BIN"

MODEL_DIR="${PLUME_MODEL_DIR:-$REPO_ROOT/plume-models}"
[[ -d "$MODEL_DIR" ]] || fail "Plume model dir not found: $MODEL_DIR"
is_checkpoint() {
  local d="$1"
  [[ -f "$d/config.json" ]] || return 1
  [[ -f "$d/tokenizer.json" || -f "$d/tokenizer.model" || -f "$d/tokenizer_config.json" ]] || return 1
  find "$d" -maxdepth 1 -type f \( -name '*.safetensors' -o -name '*.gguf' -o -name '*.npz' \) | grep -q .
}
mapfile -t qwen_dirs < <(find "$MODEL_DIR" -maxdepth 2 -type d -iname '*qwen*' 2>/dev/null | sort)
MODEL_FOLDER=""
for d in "${qwen_dirs[@]}"; do
  base="$(basename "$d")"
  if [[ "$base" =~ [Cc]oder && "$base" =~ 3[Bb] && "$base" =~ 4 ]] && is_checkpoint "$d"; then
    MODEL_FOLDER="$d"; break
  fi
done
if [[ -z "$MODEL_FOLDER" ]]; then
  for d in "${qwen_dirs[@]}"; do is_checkpoint "$d" && { MODEL_FOLDER="$d"; break; }; done
fi
[[ -z "$MODEL_FOLDER" ]] && fail "No classifiable Qwen checkpoint under $MODEL_DIR."
echo "[OK]   Qwen checkpoint: $MODEL_FOLDER"

# ─── Step 2: seed the temp fixture ───────────────────────────────────────

WORKDIR="$(mktemp -d -t plume-propose-diff.XXXXXX)"
FIXTURE="$WORKDIR/fixture"
mkdir -p "$FIXTURE"
SEED='def greet(name):
    return "Hello, " + name
'
printf '%s' "$SEED" > "$FIXTURE/greet.py"
echo "[OK]   seeded fixture: $FIXTURE/greet.py"
echo ""

# ─── Step 3: start mlx-lm and ask for a diff ─────────────────────────────

PORT=$("$PYTHON_BIN" -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()")
LOG_FILE="$(mktemp -t plume-propose-diff-log.XXXXXX)"
"$PYTHON_BIN" -c 'import os, sys; os.setsid(); os.execvp(sys.argv[1], sys.argv[1:])' \
  "$PYTHON_BIN" -m mlx_lm server --model "$MODEL_FOLDER" --host 127.0.0.1 --port "$PORT" --log-level INFO \
  >"$LOG_FILE" 2>&1 &
SERVER_PID=$!
echo "[..]   mlx-lm pid=$SERVER_PID port=$PORT — waiting for /health…"

deadline=$(( $(date +%s) + STARTUP_TIMEOUT ))
healthy=0
while [[ $(date +%s) -lt $deadline ]]; do
  if curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/health" 2>/dev/null | grep -q '^200$'; then
    healthy=1; break
  fi
  sleep 0.5
done
[[ $healthy -ne 1 ]] && fail \
  "mlx-lm did not become healthy within ${STARTUP_TIMEOUT}s." \
  "Last server output:" "$(tail -n 30 "$LOG_FILE")"
echo "[OK]   /health responded 200"

# Build the request. We show the model the current file and ask for a
# diff only. Low temperature for a more deterministic edit.
REQUEST=$(
  PLUME_MODEL="$MODEL_FOLDER" PLUME_SEED="$SEED" PLUME_INSTR="$PROMPT_INSTRUCTION" PLUME_MAXTOK="$MAX_TOKENS" \
  "$PYTHON_BIN" -c "
import json, os
content = (
    'Here is greet.py:\n\n' + os.environ['PLUME_SEED'] + '\n' + os.environ['PLUME_INSTR']
)
print(json.dumps({
    'model': os.environ['PLUME_MODEL'],
    'stream': False,
    'temperature': 0.0,
    'max_tokens': int(os.environ['PLUME_MAXTOK']),
    'messages': [{'role': 'user', 'content': content}],
}))
"
)
RESPONSE=$(curl -s -X POST "http://127.0.0.1:$PORT/v1/chat/completions" \
  -H 'Content-Type: application/json' -d "$REQUEST" || true)
[[ -z "$RESPONSE" ]] && fail "Empty chat response." "$(tail -n 30 "$LOG_FILE")"

# Extract assistant content and strip an optional ```diff fence.
DIFF_FILE="$WORKDIR/model.diff"
if ! printf '%s' "$RESPONSE" | "$PYTHON_BIN" -c "
import json, sys, re
payload = json.loads(sys.stdin.read())
content = payload['choices'][0]['message']['content']
# Strip a surrounding code fence if the model added one.
m = re.search(r'\`\`\`(?:diff|patch)?\s*\n(.*?)\n\`\`\`', content, re.S)
sys.stdout.write(m.group(1) if m else content)
" > "$DIFF_FILE" 2>/dev/null; then
  fail "Could not parse the model response as OpenAI JSON." "$(printf '%s' "$RESPONSE" | head -c 600)"
fi

echo "[OK]   captured model diff ($(wc -l < "$DIFF_FILE" | tr -d ' ') lines)"
echo "------- model diff -------"
sed 's/^/  /' "$DIFF_FILE"
echo "--------------------------"
echo ""

# ─── Step 4: run it through Plume's real validate → apply → revert ───────

echo "[..]   running Plume's patch cycle on the fixture…"
echo ""
if PLUME_SMOKE_FIXTURE="$FIXTURE" PLUME_SMOKE_DIFF_FILE="$DIFF_FILE" \
  ./scripts/dev-env.sh bash -lc "cd src-tauri && cargo test --bin plume -- --ignored --exact --nocapture patch::propose_diff_smoke_tests::qwen_propose_diff_smoke"; then
  echo ""
  echo "============== PROPOSE-DIFF: PASS =============="
  echo "  model:   $MODEL_FOLDER"
  echo "  The local Qwen diff validated, applied to the fixture, and reverted cleanly."
  echo "==============================================="
  exit 0
else
  fail \
    "The model's diff did not survive Plume's validate → apply → revert cycle." \
    "This is usually MODEL QUALITY (malformed or non-applying diff), not a bug." \
    "The diff is shown above; the Rust outcome (Invalid / ApplyFailed / …) is in the test output." \
    "No real source files were touched — only the temp fixture, now cleaned up."
fi
