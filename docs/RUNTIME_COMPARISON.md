# Runtime Comparison

A clean-room read of the local-inference landscape from Plume's
specific perspective: a Tauri desktop app for Apple Silicon, where
the model runs on the user's own machine and the editor + agent
loop run alongside it. Nothing here is copied from upstream source
trees — every claim is from the runtime's public README / docs and
verified against behavior we have observed during D27 / D36 / D40 /
D45 / D52.

If a behavior is **not** confirmed below, treat it as "we believe
this is roughly true" rather than "this is contract." File an issue
if you find a mismatch.

## Audience

Read this before adding a new runtime adapter or before answering
"why isn't Plume using `<thing>`?" in a design doc.

## The runtimes we discuss

- **MLX-LM** — Apple's MLX framework's reference inference server.
- **llama.cpp** — C++ inference engine with GGUF weights and Metal /
  CUDA / CPU back-ends; ships `llama-server` as its HTTP layer.
- **vLLM** — Python inference engine + server optimized for
  high-throughput batched serving on NVIDIA GPUs (PagedAttention,
  continuous batching).
- **Ollama** — a wrapper around llama.cpp + a model registry + a
  daemon + a CLI. Treats local inference like a package manager.
- **LM Studio** — a closed-source desktop app that bundles a chat
  UI, model browser, and multi-engine inference (MLX, llama.cpp).

We name **Locally AI** and **Hermes** only as on-disk locations
Plume's D50 scanner reads (Locally AI's HuggingFace cache). They
are end-user macOS apps in the same product space as Plume; we do
not copy their code or runtime decisions.

## Plume's primary axes

Picking a runtime for Plume means weighing five things at once.
None of them are dominated by raw tokens / sec on a press release.

1. **Hardware honesty.** Plume's headline target is Apple Silicon
   laptops with 8–64 GB of unified memory. Whatever runtime we ship
   on the happy path must run there well, today.
2. **Operator surface area.** The user installs Plume, not a runtime
   matrix. If a runtime needs CUDA, a specific Python, a daemon,
   model conversion, or shell rc edits, that cost lands on the
   user.
3. **Editor co-residency.** Plume is a desktop app, not a server
   client. The runtime has to share the host with the editor,
   CodeMirror, the file watcher, and a future agent loop. A
   runtime that needs 80 % of system RAM is a runtime that crashes
   the editor.
4. **Quality-of-output for code.** Different model families and
   different quantization shapes change how usable the same
   parameter count is. Apple Silicon happens to be where the MLX
   community has put weight on aggressive low-precision releases
   (e.g. `mlx-community/*-4bit`) that are tuned for the
   architecture.
5. **Honest fallback.** If the primary runtime can't help, the user
   should still be able to bring their own (Ollama, llama.cpp,
   LM Studio's models cache). Plume's provider track is built so
   the happy path stays uncoupled from the compatibility path.

## At-a-glance table

| Runtime    | Primary host        | Weight format             | Ships own server? | Apple Silicon support | Plume status today                                              |
| ---------- | ------------------- | ------------------------- | ----------------- | --------------------- | --------------------------------------------------------------- |
| MLX-LM     | Apple Silicon       | MLX-converted safetensors | `python -m mlx_lm server` | First-class, Metal + unified memory | **Primary Plume-managed runtime** (D40 supervisor + D45 chat)   |
| llama.cpp  | Anywhere            | GGUF                      | `llama-server`     | Metal back-end works  | Connected provider via `/v1/models` (D4); supervisor is roadmap |
| Ollama     | Anywhere            | GGUF (vendored llama.cpp) | Its own HTTP daemon | Yes (via llama.cpp)  | Connected provider via `/api/tags` (D2+); blob store is opaque  |
| vLLM       | NVIDIA-class server | safetensors / HF caches   | `vllm serve`                         | Experimental CPU (build-from-source, FP32/FP16); community `vllm-metal` plugin uses MLX as the compute backend | **Not a Plume runtime.** See § Where vLLM might help later      |
| LM Studio  | Desktop user        | GGUF + MLX                | Embedded server (OpenAI-compat) | Yes                  | Connected provider via `/v1/models` (D4); models cache scanned (D50) |

The "Plume status today" column is the part that drifts; treat the
other columns as durable runtime properties.

## Why MLX-LM is Plume's primary runtime today

The product brief in `docs/PLUME_PROJECT_SPEC.md` and the direction
doc in `docs/LOCAL_AGENT_NORTH_STAR.md` already say "MLX-first." The
reasons below are the underlying answer to "why."

1. **Unified memory makes Apple Silicon different.** On a desktop
   GPU, model weights live in VRAM and tensors round-trip across
   PCIe; on Apple Silicon, GPU cores read directly from system RAM.
   A runtime built around that — MLX is — gets to skip a class of
   copies and memory-mapping tricks that other engines added to
   work around discrete-GPU constraints. The user doesn't notice
   this directly. They notice that a 7B model fits and runs.
2. **The model community is publishing MLX-format weights.**
   `mlx-community` on HuggingFace ships 4-bit and 8-bit MLX
   conversions of every model that matters for code generation
   (Qwen2.5-Coder, DeepSeek-Coder, Gemma, Llama-3.1, etc.) within
   days of the source release. Plume can point a user at one of
   those folders and have it work without the user touching a
   quantization tool.
3. **The runtime surface is small and honest.** `python -m mlx_lm
   server` is a one-line spawn. It uses an OpenAI-compatible HTTP
   surface (SSE streams included). Plume's D40 supervisor + D39
   SSE parser + D45 chat adapter together are ~600 lines of Rust.
   We can read and reason about every byte of that path.
4. **It is not Ollama.** Not because Ollama is bad — Plume still
   talks to Ollama via `/api/tags` and routes chat through it
   (D7+) — but because Plume should not require an Ollama install
   to be useful out of the box. The headline product is "open a
   project, chat with a local model, accept its diff." If that
   path goes through a third-party daemon the user has to install,
   the product is one layer of friction worse than it needs to be.

## Where llama.cpp fits

`llama.cpp` is the inference engine the rest of this list is built
on top of (Ollama vendors it; LM Studio ships it as one of its
back-ends). It ships its own HTTP layer as `llama-server`.

Plume connects to a user-started `llama-server` via the OpenAI-style
`/v1/models` probe (D4) and lists what it sees in the provider
panel. **Chat routing for llama-server is not yet wired** — the
adapter is straightforward (same SSE shape as MLX-LM and the OpenAI
API), and the supervisor work (analogue of D40 for `llama-server`)
is on the roadmap. The reason it isn't done already is prioritization:
MLX is Plume's headline runtime, and pulling llama.cpp into the
primary path before MLX is solid would have meant shipping two
half-finished pipelines.

GGUF models — single files in a content-defined format — are also
what `providers.localModels` (D27 / D36) classifies as `gguf`. The
file is visible in the panel; Plume won't try to launch
`llama-server` on it yet.

## Where Ollama fits

Ollama is Plume's **compatibility provider**, not the happy path.
Plume detects a running Ollama daemon (D1), reads its tag catalog
(D2), shows fit estimates per model (D3), and can route chat
through it (D7+).

What Ollama gives Plume:

- A model the user already pulled is one click away from a chat.
- Tag-based UX hides the GGUF + quantization detail from users
  who don't want to think about it.
- The daemon's HTTP API is stable and well-documented.

What Ollama does not give Plume:

- Importable model files. The blob store under `~/.ollama/models/`
  is content-addressed; the human-readable model id lives only in
  Ollama's SQLite manifest. Plume's D50 inventory deliberately
  does **not** treat the blob store as a local-model source — see
  `docs/IPC_CONTRACT.md § providers.localModels` for the rationale.
- An MLX path. Ollama is GGUF on top of llama.cpp's Metal back-end,
  which is a different code path with different memory and quality
  characteristics than MLX-LM.
- A clean dependency story. Ollama is one more daemon the user has
  to install and keep running. For users who already have it, this
  is a non-cost; for users who don't, asking them to install a 1+
  GB daemon to use Plume would be the wrong default.

## Where vLLM might help Plume later

`vLLM` is excellent at what it does — high-throughput, batched,
multi-tenant LLM serving on NVIDIA GPUs. It is **not a local-laptop
inference engine**. Its design points are:

- **PagedAttention.** A KV-cache scheme that lets many concurrent
  requests share GPU memory without copying. Relevant when N
  requests are in flight at once on the same GPU.
- **Continuous batching.** Streaming generations from many
  concurrent users are merged into the same GPU batch every
  decode step.
- **NVIDIA-first.** vLLM's optimizations target CUDA and modern
  data-center GPUs (A100 / H100 / H200). Apple Silicon is reachable
  two ways today, neither of which changes the calculus for Plume:
  upstream has an *experimental* CPU build (build-from-source,
  FP32/FP16 only — no Metal acceleration), and a community plugin
  called `vllm-metal` exposes Metal by using **MLX as its compute
  backend**. In other words, the Apple Silicon path through vLLM
  ends up running MLX under the hood — so there's no engine
  advantage over the MLX-LM server Plume already supervises, only
  an extra serving layer on top.

Where this could help Plume **later**:

1. **Self-hosted agent farms.** If a Plume user wants to point the
   agent loop at a beefier remote inference endpoint (e.g. a Linux
   box with a couple of consumer GPUs they already own), vLLM is
   the obvious choice on that endpoint. Plume would consume it via
   the same OpenAI-compatible `/v1/chat/completions` adapter the
   MLX-LM and llama-server adapters use. No new client code
   required; just a "remote endpoint" provider entry.
2. **Multi-agent workloads.** Once the agent loop is real, an
   agent that spawns N sub-agents to read N files in parallel
   benefits from the server batching N requests onto one GPU.
   That's a continuous-batching workload, and that's what vLLM
   is good at.
3. **Tool-call-heavy sessions.** Long structured-output sessions
   with many short generations (one per tool call) batch better
   on a server than on a single-laptop runtime.

What vLLM does **not** unlock for Plume:

- Better single-user throughput on a MacBook. The Apple Silicon
  paths are experimental or community-maintained — either CPU-only
  upstream or MLX-backed via `vllm-metal` — so on Plume's primary
  target a vLLM session would be either slower (CPU) or the same
  engine as MLX-LM with an extra serving layer (vllm-metal). The
  gain over MLX-LM for a one-request-at-a-time editor session is
  unlikely to justify the install cost.
- Local code-completion latency. The first-token latency of a
  single request is what matters for editor completion, and
  vLLM's batching wins don't apply to that workload.

## Where LM Studio fits

LM Studio is interesting because it overlaps Plume's product space
without being a runtime Plume wraps. It ships:

- A chat UI (Plume has its own).
- A model browser (Plume's local-models panel + future
  download verb).
- An embedded OpenAI-compatible server (Plume probes it via D4).
- Multi-engine inference (MLX + llama.cpp via embedded engines).

Plume reads LM Studio's `~/.lmstudio/models/` tree (D50) so models
the user already downloaded through LM Studio surface in Plume's
panel. We do **not** drive LM Studio's runtime; the user starts the
LM Studio server when they want it on and Plume probes it as a
connected provider.

## Decision rules

Use these as the default answer when someone asks "should Plume
support X?":

- **Add a new Apple Silicon runtime adapter** (a non-MLX engine
  optimized for Metal): only if it has a real user advantage over
  MLX-LM at Plume's scale (single user, code-generation prompts,
  unified memory budget). "It's slightly faster" is not enough.
- **Add a new connected-provider adapter** (an existing daemon
  with a public API): yes, if the daemon is widely deployed and
  the adapter is < 500 lines. llama-server, Tabby, and remote
  vLLM endpoints all qualify.
- **Replace MLX-LM as the Plume-managed runtime**: no, today.
  Revisit if and when MLX-LM upstream stops keeping up with the
  open-weight code-generation models the Plume user base actually
  runs.
- **Bundle a runtime binary inside Plume.app**: no, today. The
  user is the source of truth for "is `mlx-lm` / `python` /
  `ollama` installed." Plume's job is to detect, not provide.

## Open questions

- Does shipping a `llama-server` supervisor (analogous to D40) help
  the long tail of users whose models are GGUF-only?
- When MLX adds CoreML / ANE acceleration paths, does that change
  the "MLX is unconditionally the primary" picture?
- Is there a remote-endpoint provider story Plume should ship
  before the agent loop lands, so users with home labs can wire
  up vLLM ahead of time?

These are worth revisiting every couple of slices. None are
blocking work today.

## Related docs

- `docs/PLUME_PROJECT_SPEC.md` — product brief and the
  Apple-Silicon-first thesis.
- `docs/LOCAL_AGENT_NORTH_STAR.md` — Plume's distillation /
  agent-memory direction and the MLX-first rationale.
- `docs/MODEL_PROVIDERS.md` — runtime category model (provider
  track vs engine track).
- `docs/MLX_RUNTIME.md` — implementation contract for the
  Plume-managed `mlx_lm.server` (D38 + D40).
- `docs/IPC_CONTRACT.md § providers` — what `providers.list`,
  `providers.health`, `providers.localModels`, and the supervisor
  verbs (D40 + D52) actually return on the wire.
