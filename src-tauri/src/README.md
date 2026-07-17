# Rust Domain Map

`lib.rs` builds the Tauri application and owns shared state. `app_commands.rs`
is the application-command allowlist source. `commands/` is the IPC edge:
handlers validate wire payloads and delegate to the domain modules below.

| Domain | Primary owners | IPC seam | Main tests | Source-of-truth docs |
| --- | --- | --- | --- | --- |
| Project trust | `src-tauri/src/project/mod.rs`, `src-tauri/src/project/trust.rs`, `src-tauri/src/safety/path.rs` | `src-tauri/src/commands/project.rs` | `src-tauri/src/commands/session_tests.rs` and module tests | [`docs/SAFETY.md`](../../docs/SAFETY.md), [`docs/IPC_CONTRACT.md`](../../docs/IPC_CONTRACT.md) |
| Display filesystem | `src-tauri/src/fs/list.rs`, `src-tauri/src/fs/read.rs`, `src-tauri/src/fs/policy.rs` | `src-tauri/src/commands/fs.rs` | module tests and frontend file-browser tests | [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md), [`docs/SAFETY.md`](../../docs/SAFETY.md) |
| Prompt assembly and explicit context | `src-tauri/src/prompts/assemble.rs`, `src-tauri/src/prompts/explicit_context.rs`, `src-tauri/src/prompts/context_manifest.rs`, `src-tauri/src/prompts/read.rs`, `src-tauri/src/prompts/redact.rs` | `src-tauri/src/commands/chat/context.rs`, `src-tauri/src/commands/chat/send.rs` | `src-tauri/src/prompts/assemble_tests.rs`, `src-tauri/src/prompts/explicit_context_tests.rs`, `src-tauri/src/commands/chat/context_tests.rs` | [`docs/IPC_CONTRACT.md`](../../docs/IPC_CONTRACT.md), [`docs/SAFETY.md`](../../docs/SAFETY.md) |
| Chat transports | `src-tauri/src/chat/stream.rs`, `src-tauri/src/chat/stream_read.rs`, `src-tauri/src/chat/mlx_lm.rs`, `src-tauri/src/chat/ollama.rs`, `src-tauri/src/chat/openai_sse.rs` | `src-tauri/src/commands/chat/` | `src-tauri/src/chat/mlx_lm_tests.rs`, `src-tauri/src/chat/ollama/streaming_tests.rs`, `src-tauri/src/chat/openai_sse_tests.rs`, `src-tauri/src/commands/chat/send_tests.rs` | [`docs/MODEL_PROVIDERS.md`](../../docs/MODEL_PROVIDERS.md), [`docs/IPC_CONTRACT.md`](../../docs/IPC_CONTRACT.md) |
| Sessions and branches | `src-tauri/src/sessions/mod.rs`, `src-tauri/src/sessions/schema.rs`, `src-tauri/src/sessions/search.rs`, `src-tauri/src/sessions/branch.rs` | `src-tauri/src/commands/sessions.rs`, `src-tauri/src/commands/session.rs` | `src-tauri/src/sessions/tests.rs`, `src-tauri/src/sessions/search_tests.rs`, `src-tauri/src/sessions/fork_tests.rs`, `src-tauri/src/sessions/rollback_tests.rs` | [`docs/IPC_CONTRACT.md`](../../docs/IPC_CONTRACT.md), [`docs/AGENT_OPERABILITY.md`](../../docs/AGENT_OPERABILITY.md) |
| Browser persistence and native runtime | `src-tauri/src/sessions/browser_workspace.rs`, `src-tauri/src/browser/runtime.rs`, `src-tauri/src/browser/restoration.rs`, `src-tauri/src/browser/evidence.rs` | `src-tauri/src/commands/browser_workspace.rs`, `src-tauri/src/commands/task_browser.rs`, `src-tauri/src/commands/task_browser_activation.rs`, `src-tauri/src/commands/browser.rs` | `src-tauri/src/sessions/browser_workspace_tests.rs`, `src-tauri/src/browser/runtime_tests.rs`, `src-tauri/src/commands/task_browser_tests.rs` | [`docs/IPC_CONTRACT.md`](../../docs/IPC_CONTRACT.md), [`docs/SAFETY.md`](../../docs/SAFETY.md), [`docs/ROADMAP.md`](../../docs/ROADMAP.md) |
| Project and user memory | `src-tauri/src/memory/store.rs`, `src-tauri/src/memory/user_store.rs`, `src-tauri/src/memory/topics.rs`, `src-tauri/src/memory/links.rs` | `src-tauri/src/commands/memory.rs` | `src-tauri/src/memory/memory_tests.rs`, `src-tauri/src/memory/user_store_tests.rs`, `src-tauri/src/prompts/explicit_context_tests.rs` | [`docs/FEATURE_INVENTORY.md`](../../docs/FEATURE_INVENTORY.md), [`docs/SAFETY.md`](../../docs/SAFETY.md) |
| Patch lifecycle | `src-tauri/src/patch/parse.rs`, `src-tauri/src/patch/validate.rs`, `src-tauri/src/patch/apply.rs`, `src-tauri/src/patch/checkpoint.rs`, `src-tauri/src/patch/revert.rs` | `src-tauri/src/commands/patch.rs` | `src-tauri/src/patch/parse_tests.rs`, `src-tauri/src/patch/apply_tests.rs`, `src-tauri/src/patch/revert_tests.rs` | [`docs/SAFETY.md`](../../docs/SAFETY.md), [`docs/IPC_CONTRACT.md`](../../docs/IPC_CONTRACT.md) |
| Providers and managed MLX | `src-tauri/src/providers/registry.rs`, `src-tauri/src/providers/local_models.rs`, `src-tauri/src/providers/mlx_lm/` | `src-tauri/src/commands/providers.rs` | `src-tauri/src/providers/local_models_tests.rs`, `src-tauri/src/providers/mlx_lm/process_tests.rs` | [`docs/MODEL_PROVIDERS.md`](../../docs/MODEL_PROVIDERS.md), [`docs/MLX_RUNTIME.md`](../../docs/MLX_RUNTIME.md) |
| Agent foundations and skills | `src-tauri/src/agent/`, `src-tauri/src/skills/` | `src-tauri/src/commands/agent.rs`, `src-tauri/src/commands/session.rs`, `src-tauri/src/commands/tools.rs`, `src-tauri/src/commands/skills.rs` | `src-tauri/src/agent/agent_tests.rs`, `src-tauri/src/agent/single_step_tests.rs`, `src-tauri/src/skills/tests.rs` | [`docs/AGENT_RUNTIME.md`](../../docs/AGENT_RUNTIME.md), [`docs/SAFETY.md`](../../docs/SAFETY.md), [`docs/TOOL_DISCLOSURE.md`](../../docs/TOOL_DISCLOSURE.md) |
| Host system | `src-tauri/src/system/` | `src-tauri/src/commands/system.rs` | `src-tauri/src/system/mod.rs` | [`docs/MODEL_PROVIDERS.md`](../../docs/MODEL_PROVIDERS.md) |

## Boundary rules

- Keep IPC handlers thin. Wire validation and trust checks happen before
  domain work; reusable behavior lives in the owning module.
- Display reads and prompt reads are different types and paths. Only
  `prompts::redact` produces prompt-safe content.
- Local and project session/memory/Browser stores are physically and
  logically separate. IPC accepts typed identity, never a caller-supplied
  database or app-data root.
- Browser native children have no prompt or tool authority. Human capture
  creates bounded immutable evidence; agent Browser actions are not shipped.
- Patch validation is rerun server-side before Apply. Checkpoints and drift
  checks remain the only shipped mutation/revert path for model diffs.
- The tool catalog describes visibility, not permission. Broad execution and
  computer-use emission remain unimplemented.
