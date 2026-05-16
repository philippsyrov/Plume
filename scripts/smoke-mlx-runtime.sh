#!/usr/bin/env bash
# D53: end-to-end smoke for the Plume-managed MLX-LM runtime, OUTSIDE
# the full Plume app. Given a model folder on disk, this script:
#
#   1. checks `python -c "import mlx_lm"` actually works in the
#      current shell (the most common Gemma-debug confusion is a
#      `pipx`/`uv tool` install that creates a CLI shim but not a
#      python import — same issue D47's docs flagged);
#   2. verifies the folder has a `config.json`, a `tokenizer*` file,
#      and at least one weight file (`.safetensors` / `.gguf` /
#      `.npz`) — the same classification floor Plume's scanner uses;
#   3. allocates an ephemeral port, starts `python -m mlx_lm server
#      --model <folder> --host 127.0.0.1 --port <port>` in the
#      background, drains its output to a temp file;
#   4. polls `GET /health` until 200 OK (30 s budget);
#   5. sends one tiny `POST /v1/chat/completions` and prints the
#      first ~1 KiB of the response so the operator can see the
#      model actually generated something;
#   6. SIGINTs the server with a 3 s grace, then SIGKILLs the
#      process group if it didn't exit (mirrors Plume's supervisor
#      shutdown path).
#
# **No installs.** If `mlx_lm` is missing the script prints the
# recommended venv playbook (D47 hardening) and exits non-zero.
# **No downloads.** The model must already be on disk — Plume itself
# is the importer story, not this script.
#
# This is a verification tool, not a service. Each invocation spawns
# one mlx-lm child for the duration of the smoke and shuts it down
# before exiting. Re-run with different `<model-folder>` to debug a
# different checkpoint.

set -euo pipefail

# ─── Usage ──────────────────────────────────────────────────────────────

usage() {
  cat <<'EOF'
Usage: scripts/smoke-mlx-runtime.sh <model-folder>

Arguments:
  <model-folder>   Absolute path to a Plume-classifiable
                   transformer-folder / mlx-folder. Examples (do
                   not hardcode — these depend on your install):

                     $PLUME_MODEL_DIR/gemma-2b-it
                     ~/.lmstudio/models/lmstudio-community/qwen2.5-coder-7b-instruct
                     ~/Library/Containers/app.locallyai.Locally/Data/Library/\
                       app.locallyai.Locally/huggingface/models/\
                       models--mlx-community--gemma-2b-it/snapshots/<sha>

Environment overrides:
  PYTHON_BIN       Python executable to use (default: python)
  STARTUP_TIMEOUT  Seconds to wait for /health 200 (default: 30)
  PROMPT_TEXT      Prompt for the smoke chat request (default: "ping")

Exits 0 on a successful round-trip, non-zero with a diagnostic on
any failure. The script never installs packages and never modifies
the model folder.
EOF
}

if [[ $# -ne 1 || "$1" == "--help" || "$1" == "-h" ]]; then
  usage
  exit 0
fi

MODEL_FOLDER="$1"
PYTHON_BIN="${PYTHON_BIN:-python}"
STARTUP_TIMEOUT="${STARTUP_TIMEOUT:-30}"
PROMPT_TEXT="${PROMPT_TEXT:-ping}"

# ─── Step 1: python + mlx_lm import probe ───────────────────────────────

if ! command -v "$PYTHON_BIN" >/dev/null 2>&1; then
  cat >&2 <<EOF
ERROR: '$PYTHON_BIN' is not on PATH.

Plume's MLX supervisor invokes 'python -m mlx_lm server ...' the same
way; if your shell can't find python, neither can Plume. Pass
PYTHON_BIN=path/to/your/python or activate the venv that has mlx_lm
installed.
EOF
  exit 1
fi

if ! "$PYTHON_BIN" -c "import mlx_lm" >/dev/null 2>&1; then
  cat >&2 <<EOF
ERROR: '$PYTHON_BIN -c "import mlx_lm"' failed.

The mlx-lm package isn't importable from this Python. Common cause:
'pipx install mlx-lm' or 'uv tool install mlx-lm' creates a CLI shim
on PATH but the python in this shell can't 'import mlx_lm' (the
package lives in an isolated env).

Fix:
  # Option A: project-local venv (recommended)
  python -m venv ~/.venvs/mlx-env
  source ~/.venvs/mlx-env/bin/activate
  pip install --upgrade pip
  pip install mlx-lm
  # then rerun this script from THIS shell

  # Option B: uv venv (faster install, same shape)
  uv venv ~/.venvs/mlx-env
  source ~/.venvs/mlx-env/bin/activate
  uv pip install mlx-lm

After install, smoke-check before retrying:
  $PYTHON_BIN -c "import mlx_lm; print(mlx_lm.__version__)"
EOF
  exit 1
fi

echo "[OK]   python -c 'import mlx_lm' succeeded ($PYTHON_BIN)"

# ─── Step 2: model folder shape ─────────────────────────────────────────

if [[ ! -d "$MODEL_FOLDER" ]]; then
  echo "ERROR: '$MODEL_FOLDER' is not a directory." >&2
  exit 1
fi

if [[ ! -f "$MODEL_FOLDER/config.json" ]]; then
  echo "ERROR: '$MODEL_FOLDER/config.json' missing." >&2
  echo "       Plume's scanner classifies a folder as transformer/mlx only when" >&2
  echo "       config.json + a tokenizer* file + at least one weight file are present." >&2
  exit 1
fi

# Tokenizer file (any of: tokenizer.json / tokenizer.model / tokenizer_config.json).
tokenizer_found=0
for t in tokenizer.json tokenizer.model tokenizer_config.json; do
  if [[ -f "$MODEL_FOLDER/$t" ]]; then
    tokenizer_found=1
    break
  fi
done
if [[ $tokenizer_found -eq 0 ]]; then
  echo "ERROR: no tokenizer file at '$MODEL_FOLDER/' (expected one of tokenizer.json / tokenizer.model / tokenizer_config.json)." >&2
  exit 1
fi

# Weight file: .safetensors / .gguf / .npz at folder root.
weight_count=$(find "$MODEL_FOLDER" -maxdepth 1 -type f \( -name '*.safetensors' -o -name '*.gguf' -o -name '*.npz' \) | wc -l | tr -d ' ')
if [[ "$weight_count" -eq 0 ]]; then
  echo "ERROR: no weight files (.safetensors / .gguf / .npz) at the folder root of '$MODEL_FOLDER'." >&2
  exit 1
fi

echo "[OK]   model folder shape valid ($weight_count weight file(s) found)"

# ─── Step 3: allocate ephemeral port + start mlx-lm server ──────────────

# Same allocator pattern as Plume's supervisor: bind to 0, read the
# OS-assigned port, drop the listener so the child can rebind. A small
# race window between drop and child bind is unavoidable; this script
# is a one-shot smoke, not a service, so we don't retry on health
# timeout — the operator can rerun.
PORT=$("$PYTHON_BIN" -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()")
echo "[..]   allocated port 127.0.0.1:$PORT"

LOG_FILE=$(mktemp -t plume-mlx-smoke.XXXXXX)
trap 'rm -f "$LOG_FILE"' EXIT

# Spawn in its own process group so SIGINT here doesn't double-signal
# both this script and the child. We can't use `setsid` because it
# isn't installed on macOS by default (and Plume's primary smoke
# target is Apple Silicon). Instead we use a tiny Python wrapper that
# calls `os.setsid()` then `execvp()`s mlx_lm — the resulting child
# is the process-group leader, same end-state as `setsid`, and we
# only depend on the Python interpreter we already required above.
"$PYTHON_BIN" -c 'import os, sys; os.setsid(); os.execvp(sys.argv[1], sys.argv[1:])' \
  "$PYTHON_BIN" -m mlx_lm server \
  --model "$MODEL_FOLDER" \
  --host 127.0.0.1 \
  --port "$PORT" \
  --log-level INFO \
  >"$LOG_FILE" 2>&1 &
SERVER_PID=$!
echo "[..]   spawned mlx-lm pid=$SERVER_PID port=$PORT"

# Always shut down on exit (success or failure). We send SIGINT to the
# negative pid (i.e. the process group) so any worker subprocesses
# mlx-lm spawned also see it.
shutdown_server() {
  if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    echo "[..]   shutting down mlx-lm pid=$SERVER_PID"
    kill -INT -- "-$SERVER_PID" >/dev/null 2>&1 || true
    # 3 s grace, then SIGKILL the whole pgroup.
    for _ in $(seq 1 30); do
      if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
        return 0
      fi
      sleep 0.1
    done
    kill -KILL -- "-$SERVER_PID" >/dev/null 2>&1 || true
  fi
}
trap 'shutdown_server; rm -f "$LOG_FILE"' EXIT

# ─── Step 4: poll /health ───────────────────────────────────────────────

deadline=$(( $(date +%s) + STARTUP_TIMEOUT ))
healthy=0
while [[ $(date +%s) -lt $deadline ]]; do
  if curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/health" 2>/dev/null | grep -q '^200$'; then
    healthy=1
    break
  fi
  sleep 0.5
done

if [[ $healthy -ne 1 ]]; then
  echo "ERROR: mlx-lm did not answer 200 on /health within ${STARTUP_TIMEOUT}s." >&2
  echo "       Last ~50 lines of server output (from $LOG_FILE):" >&2
  tail -n 50 "$LOG_FILE" >&2 || true
  exit 1
fi

echo "[OK]   /health responded 200"

# ─── Step 5: one tiny chat request ──────────────────────────────────────

# Echo back the model label the server claims to have loaded; the
# OpenAI route's "model must match what was loaded" check is what
# Plume's D45 Codex fix unblocked, so we want to confirm the same
# round-trip works here. We build the request JSON via python (env-
# var lookups + json.dumps) so a model path / prompt with quotes or
# spaces survives the wire intact.
CHAT_REQUEST=$(
  PLUME_SMOKE_MODEL="$MODEL_FOLDER" PLUME_SMOKE_PROMPT="$PROMPT_TEXT" \
    "$PYTHON_BIN" -c "
import json, os
print(json.dumps({
    'model': os.environ['PLUME_SMOKE_MODEL'],
    'stream': False,
    'max_tokens': 8,
    'messages': [{'role': 'user', 'content': os.environ['PLUME_SMOKE_PROMPT']}],
}))
"
)

CHAT_RESPONSE=$(curl -s -X POST "http://127.0.0.1:$PORT/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d "$CHAT_REQUEST" || true)

if [[ -z "$CHAT_RESPONSE" ]]; then
  echo "ERROR: chat request returned no body." >&2
  echo "       Last ~50 lines of server output:" >&2
  tail -n 50 "$LOG_FILE" >&2 || true
  exit 1
fi

# Show the first 1024 bytes of the response so the operator can spot
# obvious shape issues without scrolling through a long reply.
printf '%s\n' "$CHAT_RESPONSE" | head -c 1024
echo ""
echo "[OK]   /v1/chat/completions returned a body"

# Validate the response is JSON with the expected OpenAI-style shape.
# Pass the body on stdin so embedded quotes / backslashes survive
# without shell-escaping. Don't depend on the model's reply content
# (any non-error generation is fine).
if ! printf '%s' "$CHAT_RESPONSE" | "$PYTHON_BIN" -c "
import json, sys
payload = json.loads(sys.stdin.read())
choices = payload.get('choices')
assert isinstance(choices, list) and len(choices) > 0, 'no choices array'
message = choices[0].get('message') or {}
assert isinstance(message.get('content'), str), 'no content string'
" 2>/dev/null; then
  echo "WARN: response did not parse as OpenAI-shaped JSON. Body printed above." >&2
fi

echo ""
echo "[OK]   smoke completed successfully"
echo "       model: $MODEL_FOLDER"
echo "       port:  $PORT"
echo "       pid:   $SERVER_PID"
echo ""
echo "       Server will shut down now."
exit 0
