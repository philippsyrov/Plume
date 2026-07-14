# Browser screenshot evidence implementation plan

**Goal:** Add bounded native Browser screenshots to the typed project context
shelf and deliver them only to an exact Ollama model that reports `vision`.

**Architecture:** WKWebView creates a visible-viewport snapshot on the main
thread. Rust stores a bounded immutable PNG plus metadata under the trusted
project. The typed context resolver carries image evidence separately from text
context. `chat.send` proves the selected Ollama model's capability before
registering a stream, then the Ollama adapter adds the PNG to the final user
message's `images` field.

## Task 1: Immutable screenshot store

- Extend `src-tauri/src/browser/evidence.rs` and its tests.
- Add strict `bs_` ids, metadata/PNG caps, atomic paired writes, symlink and
  hardlink refusal, read-time PNG signature/dimension/size validation, and
  sanitized URL/title provenance.
- Run `cargo test browser::evidence_tests -- --nocapture`.

## Task 2: Native visible-viewport capture

- Add macOS-only objc2 dependencies and a small native snapshot helper.
- Extend `commands/browser.rs` with a main-webview-only screenshot command.
- Bind the callback to Browser page generation plus project identity before
  storage. Unsupported platforms return a typed blocked result.
- Test command boundary helpers and compile the real macOS path.

## Task 3: Typed image context and vision gate

- Add `browserScreenshotEvidence` to Rust/TypeScript refs, validation, identity,
  session persistence, preview outcomes, and exact manifests.
- Keep PNG bytes out of text-system-message assembly and its byte budget.
- Reuse Ollama `/api/show` to require exact `vision` support before stream-id
  registration; MLX rejects synchronously.
- Extend the Ollama streaming request builder so only the final user message
  receives the base64 PNG `images` array.
- Add regressions for non-vision, probe failure, stale/missing/tampered evidence,
  exact manifest parity, and unchanged text-only JSON.

## Task 4: Human UI

- Add `Use screenshot in chat` to the Browser panel.
- Reuse the existing capture generation and project-scope handoff behavior.
- Render screenshot provenance on the context shelf and plain blocked copy for
  non-vision models; do not send or display raw PNG through frontend IPC.
- Add API, hook, Browser panel, shelf, send, persistence, and race tests.

## Task 5: Proof and integration

- Run focused Rust/frontend tests, fmt, clippy, typecheck, and diff checks.
- Update IPC, safety, Browser, UI, feature inventory, development, and smoke
  docs with shipped/candidate status kept exact.
- Run `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`.
- Build the packaged smoke app and physically prove visible-viewport capture,
  shelf persistence, non-vision blocking, and vision request shape where a
  local vision model/runtime is available. Do not claim unavailable hardware
  evidence.
- Push one coherent PR, wait for GitHub verify and gitleaks, obtain an
  independent exact-head review, fix genuine findings, reverify, and merge.
