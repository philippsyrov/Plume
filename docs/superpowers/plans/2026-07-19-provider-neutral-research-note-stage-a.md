# Provider-Neutral Research Note Stage A Implementation Plan

> **For Codex:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to
> implement this plan task by task. Use `superpowers:test-driven-development`
> for every behavior change and `superpowers:verification-before-completion`
> before each commit/PR claim.

**Goal:** Let Qwen/MLX and Apple On-Device use one Plume-owned, bounded harness
to turn explicitly attached Browser text evidence into a cited Markdown note,
stage it under the exact chat session, preview it inertly, and export it only
through a native Save dialog.

**Architecture:** Extend the existing pure `agent::controller::run_loop` with a
production research step adapter. Rust owns source resolution, budgets,
provider framing, retries, citations, persistence, events, cancellation, and
export. Provider ports only translate model turns. React holds opaque ids and
renders the typed state. Stage A has no search, URL fetch, Browser control,
shell, arbitrary write, semantic retrieval, or non-model network authority.

**Tech stack:** Rust 2021, Tauri 2, existing blocking MLX/Apple transports,
Swift 6 `FoundationModels`, React 19, strict TypeScript, Vitest, Rust/Swift
tests, and `objc2-app-kit` for the native Save panel.

**Design source:**
[`docs/superpowers/specs/2026-07-19-provider-neutral-research-artifact-harness-design.md`](../specs/2026-07-19-provider-neutral-research-artifact-harness-design.md)

## Review decisions locked into this plan

- `13` is the logical-workflow ceiling: ten summaries, one synthesis, and two
  citation repairs. Each logical turn has one recovery allowance, used by
  either malformed-framing re-ask or context-overflow repack. The absolute
  provider-call ceiling is `26`.
- A malformed response executes nothing. The sole re-ask contains a bounded
  parse diagnostic and the exact expected schema. A second malformed response
  fails closed.
- The installed macOS SDK exposes `SystemLanguageModel.contextSize` from
  macOS 26 and exact token counting from macOS 26.4. Both must be availability
  guarded. Conservative estimation is the expected macOS 26.0–26.3 path.
- Research resolves each eligible Browser text record through its owning
  source store. It must not call the chat aggregate resolver, whose 256 KiB
  total cap is intentionally different.
- `Draft — citations need review` is an ordinary successful terminal with
  honest warnings, not an exceptional error.
- `NSSavePanel` runs only on the macOS main thread.
- “No network” means no non-model-transport network I/O. MLX loopback HTTP and
  the bounded Apple helper are provider transport.

## Task 1: Add the strict provider-neutral tool protocol

**Files:**

- Create: `src-tauri/src/agent/protocol.rs`
- Create: `src-tauri/src/agent/protocol_tests.rs`
- Modify: `src-tauri/src/agent/mod.rs`

**Step 1: Write failing parser tests**

Pin one exact text envelope:

```text
<plume_tool_call>{"callId":"c1","tool":"research.summary.submit","arguments":{"sourceId":"S1","summary":"..."}}</plume_tool_call>
```

Test exact acceptance plus rejection of missing/duplicate envelopes, prose
outside the envelope, unknown tools/fields, invalid JSON, oversized replies,
wrong phase arguments, duplicate terminal records, and control characters in
ids. Assert every rejection returns a bounded typed `ProtocolErrorCode` and
never a partially accepted call.

Run:

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test agent::protocol::tests -- --nocapture'
```

Expected: FAIL because the module does not exist.

**Step 2: Implement the minimal strict protocol**

Add `ToolCall`, the two Stage A argument unions, a deny-unknown-fields wire
shape, a 256 KiB response cap, and phase-aware validation. Keep parser errors
machine-readable and user-safe; never echo raw model output.

Add helpers that build:

- the disclosed schema/instructions;
- Qwen/ChatML-compatible framing text;
- Apple instructions-channel framing text; and
- the one-recovery re-ask from `ProtocolErrorCode`.

The two adapters share the same parser and internal `ToolCall`.

**Step 3: Run focused tests and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test agent::protocol::tests -- --nocapture'
git add src-tauri/src/agent/mod.rs src-tauri/src/agent/protocol.rs src-tauri/src/agent/protocol_tests.rs
git commit -m "feat: add strict research tool protocol"
```

## Task 2: Make logical and physical call budgets unambiguous

**Files:**

- Create: `src-tauri/src/research/mod.rs`
- Create: `src-tauri/src/research/budget.rs`
- Create: `src-tauri/src/research/budget_tests.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Write failing budget tests**

Test these invariants:

- at most 13 logical turns;
- at most one recovery per logical turn;
- malformed retry and overflow repack compete for the same allowance;
- at most 13 recoveries and 26 provider calls globally;
- a rejected recovery does not execute a provider call; and
- all counters use checked/saturating accounting and serialize exactly.

Run the missing suite and confirm red.

**Step 2: Implement `ResearchBudget`**

Keep all ceilings in Rust constants. Expose `begin_logical_turn`,
`reserve_provider_call`, and `reserve_recovery(reason)` methods that return
typed refusal reasons rather than booleans. Do not overload the existing
session `iterationCap`; this workflow owns a fixed smaller safety budget.

**Step 3: Run and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::budget::tests -- --nocapture'
git add src-tauri/src/lib.rs src-tauri/src/research
git commit -m "feat: bound research workflow calls"
```

## Task 3: Add provider capability reporting and reusable model-turn ports

**Files:**

- Modify: `src-tauri/apple-model/Sources/PlumeAppleModel/Protocol.swift`
- Modify: `src-tauri/apple-model/Sources/PlumeAppleModel/Generation.swift`
- Modify: `src-tauri/apple-model/Sources/PlumeAppleModel/main.swift`
- Modify: `src-tauri/apple-model/Tests/PlumeAppleModelTests/ProtocolTests.swift`
- Modify: `src-tauri/apple-model/Tests/PlumeAppleModelTests/GenerationTests.swift`
- Modify: `src-tauri/src/providers/apple_foundation.rs`
- Modify: `src-tauri/src/providers/apple_foundation_tests.rs`
- Modify: `src-tauri/src/chat/apple_foundation.rs`
- Modify: `src-tauri/src/chat/apple_foundation_tests.rs`
- Modify: `src-tauri/src/chat/mlx_lm.rs`
- Modify: `src-tauri/src/chat/mlx_lm_tests.rs`
- Create: `src-tauri/src/research/model.rs`
- Create: `src-tauri/src/research/model_tests.rs`
- Modify: `src-tauri/src/research/mod.rs`

**Step 1: Pin the Apple availability matrix in failing Swift tests**

Add a `capabilities` helper mode returning bounded JSON with `contextSize` and
`exactTokenCountAvailable`. Add optional prompt-token telemetry to the terminal
generation record. Compile/runtime guard token counting with `#available(macOS
26.4, *)`; keep `contextSize` available on the macOS 26 deployment target.

Tests use a fake capability/session seam and must cover:

- macOS 26 context size with unavailable exact counting;
- exact count when available;
- count failure becoming `nil`, not generation failure; and
- bounded output/unknown-field rejection.

Run:

```bash
./scripts/dev-env.sh swift test --package-path src-tauri/apple-model
```

Expected: FAIL before implementation.

**Step 2: Refactor both transports around a collector callback**

Extract the already-bounded MLX and Apple generation loops so chat can still
emit streaming deltas while research can collect one capped response. Do not
duplicate sockets, deadlines, cancellation, helper reaping, or error mapping.
Keep all pre-existing chat tests green.

**Step 3: Implement the research model port**

Define a private `ResearchModelPort` returning:

```rust
ModelCapabilities { context_tokens, exact_token_count }
ModelTurnResult { text, prompt_tokens, output_tokens, finish }
```

Add Qwen/MLX and Apple implementations selected by the existing provider/model
identity. Qwen requires the owned MLX `handleId`; Apple is handleless. The
port accepts already-packed messages and cancellation only. It has no source,
permission, persistence, or tool authority.

Use a conservative estimator whenever exact token count is absent. Keep the
fallback constant documented and tested rather than reading unverified model
metadata.

**Step 4: Verify and commit**

```bash
./scripts/dev-env.sh swift test --package-path src-tauri/apple-model
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test apple_foundation -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test mlx_lm -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::model::tests -- --nocapture'
git add src-tauri/apple-model src-tauri/src/chat src-tauri/src/providers src-tauri/src/research
git commit -m "feat: expose bounded research model turns"
```

## Task 4: Resolve exact Browser evidence without the chat aggregate cap

**Files:**

- Create: `src-tauri/src/sessions/owner.rs`
- Create: `src-tauri/src/sessions/owner_tests.rs`
- Modify: `src-tauri/src/sessions/mod.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/commands/chat/context_tests.rs`
- Modify: `src-tauri/src/commands/chat/send_tests.rs`
- Create: `src-tauri/src/research/evidence.rs`
- Create: `src-tauri/src/research/evidence_tests.rs`
- Modify: `src-tauri/src/research/mod.rs`

**Step 1: Extract and test reusable session ownership**

Move the wire-neutral local/project owner identity and existence check out of
the chat command module. Preserve chat behavior byte-for-byte. Test local and
project store separation, missing owners, scope mismatch, and project trust.

**Step 2: Write failing research resolver tests**

The research payload accepts only ordered Browser text evidence ids. Test:

- 1–10 records preserve order and mint `S1`…`S10`;
- screenshot, file, memory, and topic refs are rejected;
- local evidence cannot resolve through a project owner or vice versa;
- a different session/project cannot reuse an evidence id;
- a valid project evidence id is refused unless it is present in the owning
  persisted session's current explicit-context shelf;
- per-source 64 KiB and total 4 MiB accounting is enforced;
- hashes, sanitized URLs, redaction counts, capture time, and truncation are
  copied from revalidated source records; and
- a set above the chat resolver's 256 KiB aggregate cap remains valid here.

**Step 3: Implement direct owning-store resolution**

For local owners call `read_local_text_evidence`; for trusted-project owners
call `browser::evidence::read_text_evidence` using the backend-resolved current
root. First load the owning session and require every requested id to appear as
a `browserTextEvidence` item in its persisted current context shelf; the
project evidence store is project-scoped on disk and that membership check is
what prevents cross-chat id reuse. Never call
`resolve_explicit_context_for_send*`. Revalidate session identity and project
generation immediately before returning the immutable run evidence vector.

**Step 4: Verify and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test sessions::owner::tests -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::evidence::tests -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test commands::chat -- --nocapture'
git add src-tauri/src/sessions src-tauri/src/commands/chat.rs src-tauri/src/commands/chat src-tauri/src/research
git commit -m "feat: resolve session-owned research evidence"
```

## Task 5: Implement provider-aware packing and map/reduce inputs

**Files:**

- Create: `src-tauri/src/research/context.rs`
- Create: `src-tauri/src/research/context_tests.rs`
- Modify: `src-tauri/src/research/mod.rs`

**Step 1: Write failing packer tests**

Test Apple 4096-token fallback, a larger Qwen budget, reserved instruction/
schema/output space, UTF-8-safe trimming, exact source ordering, visible
truncation, synthesis from summaries only, one smaller repack, and refusal to
silently omit a source.

Also test that recovery accounting is shared: an overflow repack consumes the
same allowance that a malformed re-ask would need.

**Step 2: Implement pure packing**

Create fresh summary messages per source and synthesis/repair messages from
bounded summaries. The packer receives capabilities and a token counter seam;
it does not call providers itself. Return a manifest describing retained
bytes/tokens and truncation for event/bundle recording.

**Step 3: Verify and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::context::tests -- --nocapture'
git add src-tauri/src/research
git commit -m "feat: pack bounded research turns"
```

## Task 6: Add deterministic citation verification and Markdown projection

**Files:**

- Create: `src-tauri/src/research/citations.rs`
- Create: `src-tauri/src/research/citations_tests.rs`
- Create: `src-tauri/src/research/markdown.rs`
- Create: `src-tauri/src/research/markdown_tests.rs`
- Modify: `src-tauri/src/research/mod.rs`

**Step 1: Write failing citation tests**

Pin `[[S1]]`…`[[S10]]`. Require at least one valid citation in every non-empty
prose paragraph/list item; exempt headings and fenced code. Reject malformed,
unknown, unaccepted, duplicate-terminal, and stale-hash references. Confirm the
verifier labels provenance only and never claims relevance/factual truth.

**Step 2: Write failing projection tests**

Rust, not the model, appends the Sources section from immutable records and
converts inline ids to ordinary Markdown footnotes. Test URL/title escaping,
stable order, no model-supplied Sources section, artifact size cap, and exact
export bytes.

**Step 3: Implement the pure modules, verify, and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::citations::tests -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::markdown::tests -- --nocapture'
git add src-tauri/src/research
git commit -m "feat: verify research citations"
```

## Task 7: Persist immutable session-owned artifact bundles

**Files:**

- Create: `src-tauri/src/research/bundle.rs`
- Create: `src-tauri/src/research/bundle_tests.rs`
- Modify: `src-tauri/src/research/mod.rs`
- Modify: `src-tauri/src/commands/sessions.rs`
- Modify: `src-tauri/src/commands/sessions_tests.rs`

**Step 1: Write failing store tests**

Use temp local/project stores. Test versioned round-trip, immutable versions,
record/byte caps, atomic publication, corruption quarantine/recovery,
single-link regular files, symlink/hardlink refusal, process locking, exact
owner scope, cross-project denial, and no partial record after failure.

Use:

- local: app-data sibling `research-artifacts/<sessionId>/`;
- project: `<root>/.plume/research-artifacts/<sessionId>/`.

**Step 2: Add session-delete cleanup tests**

Deleting a local/project session must tombstone its artifact directory before
the database commit, restore it if deletion fails, and purge it after success.
Reconcile interrupted tombstones on the next store access. Do not weaken the
existing Browser-evidence cleanup.

**Step 3: Implement, verify, and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::bundle::tests -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test commands::sessions -- --nocapture'
git add src-tauri/src/research src-tauri/src/commands/sessions.rs src-tauri/src/commands/sessions_tests.rs
git commit -m "feat: store session-owned research artifacts"
```

## Task 8: Drive the real bounded harness and event stream

**Files:**

- Create: `src-tauri/src/agent/harness.rs`
- Create: `src-tauri/src/agent/harness_tests.rs`
- Modify: `src-tauri/src/agent/mod.rs`
- Modify: `src-tauri/src/agent/events.rs`
- Modify: `src-tauri/src/agent/events_tests.rs`
- Create: `src-tauri/src/research/run.rs`
- Create: `src-tauri/src/research/run_tests.rs`
- Modify: `src-tauri/src/research/mod.rs`

**Step 1: Extend events with failing wire tests**

Add generic typed progress/recovery/artifact/terminal events without exposing
source bodies or raw model output. Include namespaced tool id, phase,
current/total, logical turns, provider calls, citation status, and bounded
diagnostics under Details. Keep monotonic `seq` and exact terminal semantics.

**Step 2: Write fake-port end-to-end tests**

Drive the same workflow through fake Qwen and Apple ports:

1. one summary call per source;
2. one synthesis call;
3. zero-to-two citation repairs;
4. staging only after terminal policy; and
5. exact event order.

Cover malformed then successful re-ask, second malformed failure, overflow
repack, competing recovery reasons, 26-call hard stop, citation exhaustion as
ordinary `needsReview`, provider failure, deadline, cancellation before/after
every boundary, stale session/project generation, store failure, and late
event suppression.

**Step 3: Implement the production step adapter**

Use `agent::controller::run_loop` as the logical-turn driver. The adapter owns
phase state and invokes the model port synchronously inside the existing Tauri
blocking pool. Register only `research.summary.submit` and
`artifact.markdown.submit`; no broad catalog/tool executor is reachable.

Add a `ResearchRunRegistry` keyed by client-minted run id with one cancel flag
and captured owner generation. Registration is duplicate-safe; terminal paths
always remove the entry. A running Apple helper is killed/reaped on cancel;
MLX observes the existing bounded cancel loop.

**Step 4: Verify and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test agent::harness::tests -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::run::tests -- --nocapture'
git add src-tauri/src/agent src-tauri/src/research
git commit -m "feat: run bounded research workflows"
```

## Task 9: Expose thin research IPC and cancellation

**Files:**

- Create: `src-tauri/src/commands/research.rs`
- Create: `src-tauri/src/commands/research_tests.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/commands/project.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app_commands.rs`
- Modify: `src-tauri/capabilities/default.json`

**Step 1: Write failing command tests**

Define and pin:

- `research.start` with client run id, owner, question, provider/model,
  optional MLX handle, and ordered Browser text evidence ids;
- `research.cancel`;
- `research.listArtifacts`;
- `research.loadArtifact`; and
- event channel `research/event` filtered by run id.

Test envelope version, deny-unknown-fields, caps, provider/handle matching,
trust, owner existence, listener-before-start race contract, duplicate ids,
cancel idempotence, load/list scope isolation, and one terminal event.

**Step 2: Wire commands and managed state**

Add `Arc<ResearchRunRegistry>` to `AppState`. Commands validate, resolve, and
spawn; domain code performs the workflow. Register every command in both the
Tauri handler and hand-maintained allowlist/capability manifest.

**Step 3: Run command/allowlist tests and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test commands::research -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test app_commands -- --nocapture'
git add src-tauri/src/commands src-tauri/src/lib.rs src-tauri/src/app_commands.rs src-tauri/capabilities/default.json
git commit -m "feat: expose research workflow IPC"
```

## Task 10: Add typed frontend orchestration with stale-run fences

**Files:**

- Create: `src/lib/api/research.ts`
- Create: `src/lib/api/research.test.ts`
- Modify: `src/lib/api/agentEvents.ts`
- Create: `src/features/research/useResearchRun.ts`
- Create: `src/features/research/useResearchRun.test.tsx`

**Step 1: Write failing API/hook tests**

Test subscribe-before-start, monotonic sequence enforcement, duplicate/gap
handling, cancel, exactly one terminal, owner/model/source payloads, session
switch cancellation, unmount cancellation, late-event suppression, artifact
reload after remount, and review-needed as non-error terminal state.

**Step 2: Implement the wrappers and reducer**

Mint run ids frontend-side like chat streams. Keep only opaque source/artifact
ids plus safe summaries. The reducer projects typed events into calm phase
rows; it never parses model prose or decides citation validity.

**Step 3: Verify and commit**

```bash
npx vitest run src/lib/api/research.test.ts src/features/research/useResearchRun.test.tsx
git add src/lib/api src/features/research
git commit -m "feat: orchestrate research runs in frontend"
```

## Task 11: Add the calm Create → Research note experience

**Files:**

- Create: `src/features/research/CreateMenu.tsx`
- Create: `src/features/research/CreateMenu.test.tsx`
- Create: `src/features/research/ResearchProgress.tsx`
- Create: `src/features/research/ResearchProgress.test.tsx`
- Create: `src/features/research/ResearchArtifactCard.tsx`
- Create: `src/features/research/ResearchArtifactCard.test.tsx`
- Create: `src/features/research/SafeMarkdownPreview.tsx`
- Create: `src/features/research/SafeMarkdownPreview.test.tsx`
- Modify: `src/features/chat/ChatPanel.tsx`
- Modify: `src/features/chat/ChatPanel.test.tsx`
- Modify: `src/features/README.md`
- Modify: `src/styles/layout/chat.css`
- Create: `src/styles/layout/research.css`
- Modify: `src/styles/layout.css`

**Step 1: Write failing interaction/accessibility tests**

Pin:

- stable **Create** and **Research note** names;
- keyboard menu behavior: arrows, Home/End, Escape, outside dismiss, focus
  restoration;
- only attached `browserTextEvidence` refs count as eligible sources;
- concise start summary with model/source count/limits;
- **Stop** always visible while active;
- polite phase announcements without token spam;
- collapsible Details and source inspection;
- **Citations verified** versus **Draft — citations need review** copy;
- review-needed remains Preview/Sources/Export eligible; and
- no claim that citations are relevant or facts are verified.

**Step 2: Implement the minimal UI using existing tokens/primitives**

Keep research in the normal composer. Reuse `Disclosure`, current context
shelf, buttons, radii, typography, and spacing. Do not add a technical
dashboard, icon asset, dependency, or new app route.

Implement a small safe Markdown block parser that returns React text nodes for
headings, paragraphs, lists, and fenced code. Never use
`dangerouslySetInnerHTML`. Images become labelled blocked rows; links remain
inert text and are opened only from explicit source controls through the
existing human Browser boundary.

**Step 3: Verify focused UI and commit**

```bash
npx vitest run src/features/research src/features/chat/ChatPanel.test.tsx
npm run typecheck
git add src/features src/styles src/lib/api/agentEvents.ts
git commit -m "feat: add calm research note workflow"
```

## Task 12: Add main-thread native Markdown export

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/research/export.rs`
- Create: `src-tauri/src/research/export_tests.rs`
- Modify: `src-tauri/src/research/mod.rs`
- Modify: `src-tauri/src/commands/research.rs`
- Modify: `src-tauri/src/commands/research_tests.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app_commands.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src/lib/api/research.ts`
- Modify: `src/lib/api/research.test.ts`
- Modify: `src/features/research/ResearchArtifactCard.tsx`
- Modify: `src/features/research/ResearchArtifactCard.test.tsx`

**Step 1: Write failing pure export tests**

Behind a dialog/file-port seam, test cancelled/saved/failed outcomes, exact
bytes, `.md` default extension, atomic replacement, overwrite refusal/consent,
symlink and multi-link refusal, temp cleanup, and unchanged staged bundle on
failure.

**Step 2: Implement `NSSavePanel` on the main thread**

Enable only compiler-required `objc2-app-kit` features. The Tauri command
schedules panel creation and `runModal` through `AppHandle::run_on_main_thread`
and receives the result through a bounded channel while the async command is
off the main thread. Never construct or message AppKit objects from the worker.

The frontend never supplies a path. It supplies only an artifact id; Rust
loads the exact owned version, obtains the user-selected URL, applies file
guards, and writes atomically. Cancellation returns focus to **Export
Markdown** and is not shown as an error.

**Step 3: Verify and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::export::tests -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo clippy --all-targets'
npx vitest run src/lib/api/research.test.ts src/features/research/ResearchArtifactCard.test.tsx
git add src-tauri src/lib/api/research.ts src/lib/api/research.test.ts src/features/research
git commit -m "feat: export staged research notes"
```

## Task 13: Update contracts, inventory, maps, and manual proof

**Files:**

- Modify: `docs/AGENT_RUNTIME.md`
- Modify: `docs/IPC_CONTRACT.md`
- Modify: `docs/SAFETY.md`
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/DECOMPOSITION.md`
- Modify: `docs/USER_GUIDE.md`
- Modify: `docs/MANUAL_TESTING.md`
- Modify: `docs/SMOKE_TESTING.md`
- Modify: `src-tauri/src/README.md`
- Modify: `src/features/README.md`
- Modify: `README.md` only if the shipped user flow is ready to claim

**Step 1: Update docs from the exact implementation**

Document Stage A as shipped only after tests/smoke prove it. State plainly:

- attached human-captured Browser text only;
- no search/fetch/Browser authority;
- two internal submit tools only;
- 13 logical / 26 provider-call ceilings;
- macOS 26 conservative Apple counting path;
- review-needed is normal and provenance is not truth;
- session-local staging and explicit native export;
- MLX loopback/Apple helper are model transport; and
- Stage B network axis and Stage C search adapters remain candidate-only.

Add ownership/test/IPC rows to both domain maps. Keep history out of current
status docs. Pin inventory evidence to the implementation commit ancestor, not
an eventual squash hash guessed in advance.

**Step 2: Run documentation/navigation checks and commit**

```bash
npm run verify:docs
git diff --check
git add README.md docs src/features/README.md src-tauri/src/README.md
git commit -m "docs: document bounded research notes"
```

## Task 14: Full verification, packaged evidence, and PR gate

**Step 1: Run the complete local battery**

```bash
./scripts/dev-env.sh swift test --package-path src-tauri/apple-model
npm run typecheck
npm run test
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test'
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
git diff --check
git status --short
```

Expected: all tests pass; only explicitly documented standing warnings remain.

**Step 2: Build the packaged app at the exact implementation head**

Use `./scripts/smoke-app.sh`. Through packaged Plume and Computer Use, record:

- Qwen: one verified note and one malformed-frame recovery;
- Apple: one note using reported context/fallback behavior;
- Stop during a model turn;
- context-overflow repack;
- one citation repair;
- ordinary review-needed fallback;
- inert preview and source inspection;
- Save cancellation and successful Markdown export;
- session/project switch stale-event fencing; and
- quit/relaunch artifact restoration.

Confirm Stage A creates no non-model network I/O, starts no non-model child
process, reaps Apple helpers, adds no idle child process, and leaves Browser
human-controlled. Record idle RSS before/after feature availability and active
peak RSS while the model runs.

**Step 3: Run git/CI/review gates**

```bash
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
git status --short
git log --oneline --decorate origin/main..HEAD
```

Then run the pre-commit/gitleaks gate, push `codex/research-artifact-harness`,
open one focused ready PR, wait for GitHub verify/gitleaks, and request a
findings-only exact-head review. Fix only verified issues with TDD. Do not merge
without the user's explicit instruction.

## Stop boundary

This plan ends after Stage A is exact-head green and externally reviewed.
Do not implement URL fetch, search providers, DOCX, slides, native Apple tools,
semantic retrieval, Browser actions, shell execution, or broader permissions
in this PR. Each requires its own design/plan/review from clean updated main.
