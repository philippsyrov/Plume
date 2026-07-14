# Browser Text Evidence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture an immutable, redacted Browser page-text or selection snapshot and place its opaque project-scoped reference onto the existing exact-manifest chat context shelf.

**Architecture:** The trusted main webview invokes one fixed capture command. Rust observes the sandbox through a constant `eval_with_callback` script, rejects stale page generations, stores a capped JSON record below `.plume/browser-evidence`, and returns only redacted metadata plus an opaque id. Preview/send resolve the stored record through a new `browserTextEvidence` context kind; the frontend reuses the existing project-chat shelf handoff.

**Tech Stack:** Tauri 2 / Rust, serde JSON, React 19 / TypeScript, Vitest, existing Plume IPC/context/session contracts.

## Global Constraints

- `browser-sandbox` receives no Tauri application or core permission.
- No script string, selector, expression, or requested URL crosses capture IPC.
- Capture requires a trusted project and a current, fully loaded Browser page.
- Selection content cap is 16 KiB; page content cap is 64 KiB; title cap is 512 UTF-8 bytes; callback cap is 512 KiB.
- Store cap is 100 records and 4 MiB; no silent eviction.
- Title and content are redacted in Rust before the command response reaches the frontend.
- Browser URLs are provenance only and never trigger a fetch.
- Local sessions continue rejecting context shelves.
- Screenshot capture, drag/drop, agent actions, automatic retrieval, and host control are out of this slice.

---

### Task 1: Immutable project-scoped Browser evidence store

**Files:**
- Create: `src-tauri/src/browser/evidence.rs`
- Create: `src-tauri/src/browser/evidence_tests.rs`
- Modify: `src-tauri/src/browser/mod.rs`

**Interfaces:**
- Consumes: `crate::prompts::redact::redact`, a trusted project root, and a bounded `CapturedBrowserText`.
- Produces: `BrowserEvidenceRecord`, `BrowserEvidenceSummary`, `store_text_evidence(root, capture)`, and `read_text_evidence(root, evidence_id)`.

- [ ] **Step 1: Write failing store tests**

Add tests for id shape, selection/page byte caps, UTF-8-safe truncation, title/content redaction, 100-record and 4 MiB caps, no eviction, symlinked `.plume`/store/final-file refusal, hardlinked prompt-read refusal, malformed/version-mismatched JSON rejection, and atomic round-trip. Use this record shape:

```rust
BrowserEvidenceRecord {
    version: 1,
    id: "be_<32 hex>",
    capture_kind: BrowserCaptureKind::Selection,
    source_url: "https://example.com/page",
    title: Some("Example"),
    captured_at_ms: 1,
    content: "selected text",
    bytes: 13,
    redaction_count: 0,
    truncated: false,
}
```

- [ ] **Step 2: Run tests and confirm the module is missing**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser::evidence_tests -- --nocapture'
```

Expected: compile failure because `browser::evidence` is not defined.

- [ ] **Step 3: Implement the store**

Define these exact public types and constants:

```rust
pub const BROWSER_SELECTION_BYTE_CAP: usize = 16 * 1024;
pub const BROWSER_PAGE_BYTE_CAP: usize = 64 * 1024;
pub const BROWSER_TITLE_BYTE_CAP: usize = 512;
pub const BROWSER_EVIDENCE_MAX_RECORDS: usize = 100;
pub const BROWSER_EVIDENCE_TOTAL_BYTE_CAP: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserCaptureKind { Selection, Page }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedBrowserText {
    pub capture_kind: BrowserCaptureKind,
    pub source_url: String,
    pub title: Option<String>,
    pub content: String,
    pub source_truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserEvidenceRecord {
    pub version: u32,
    pub id: String,
    pub capture_kind: BrowserCaptureKind,
    pub source_url: String,
    pub title: Option<String>,
    pub captured_at_ms: u64,
    pub content: String,
    pub bytes: u64,
    pub redaction_count: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserEvidenceSummary {
    pub evidence_id: String,
    pub capture_kind: BrowserCaptureKind,
    pub source_url: String,
    pub title: Option<String>,
    pub captured_at_ms: u64,
    pub bytes: u64,
    pub redaction_count: u64,
    pub truncated: bool,
    pub preview: String,
}
```

Use `OnceLock<Mutex<()>>` around capacity scan plus creation. Resolve only
`<root>/.plume/browser-evidence/<validated-id>.json`; refuse symlinks at every
component, require regular files with Unix link count 1 on reads, serialize one
record per file, and write through a sibling tempfile plus atomic rename.

- [ ] **Step 4: Run store tests**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser::evidence_tests -- --nocapture'
```

Expected: all evidence-store tests pass.

- [ ] **Step 5: Commit the store**

```bash
git add src-tauri/src/browser/mod.rs src-tauri/src/browser/evidence.rs src-tauri/src/browser/evidence_tests.rs
git commit -m "feat: add browser evidence store"
```

### Task 2: Fixed capture observation command and stale-page guard

**Files:**
- Modify: `src-tauri/src/browser/state.rs`
- Modify: `src-tauri/src/commands/browser.rs`
- Modify: `src-tauri/src/app_commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`

**Interfaces:**
- Consumes: Task 1 store and a command-local `trusted_open(&AppState)` helper mirroring the existing memory/patch trust gate.
- Produces: `browser_sandbox_capture_text(IpcRequest<BrowserCaptureTextPayload>) -> BrowserEvidenceSummary` callable by `main` only.

- [ ] **Step 1: Write failing state/command tests**

Pin this ticket interface:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserCaptureTicket { pub generation: u64, pub current_url: String }

pub fn capture_ticket(&self) -> Result<BrowserCaptureTicket, IpcError>;
pub fn capture_ticket_is_current(&self, ticket: &BrowserCaptureTicket) -> bool;
```

Tests must reject closed/loading/failed state, reject tickets after navigation or
window replacement, accept the same finished page, pin the fixed script string,
reject non-main callers, and assert command registration/capability parity.

- [ ] **Step 2: Run focused tests and confirm failure**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test commands::browser -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test app_commands -- --nocapture'
```

Expected: compile failures for the missing ticket and command.

- [ ] **Step 3: Implement the fixed observation**

Add Rust-owned selection/page IIFEs. Each reads its source once, bounds the
untrusted string before returning, and returns only this object:

```javascript
(() => {
  const raw = String(window.getSelection?.()?.toString() || '');
  const sourcePrefix = raw.slice(0, 262144);
  const bytes = new TextEncoder().encode(sourcePrefix);
  const capped = bytes.subarray(0, 16384);
  return {
    url: String(location.href),
    title: String(document.title || '').slice(0, 2048),
    content: new TextDecoder().decode(capped),
    truncated: raw.length > sourcePrefix.length || bytes.length > capped.length
  };
})()
```

Do not accept JavaScript from the request. Parse the callback only when its raw
JSON is at most 128 KiB. Require `main`, a trusted project, a capture ticket, and
the sandbox window. Await the callback through a bounded channel, require the
returned URL to equal the ticket URL, re-check the ticket, reject empty
selection with `BadArgument("browser.emptySelection")`, and pass the chosen
content to `store_text_evidence`. The page IIFE is identical except it reads
`document.body?.innerText` and caps at 65,536 bytes. Reject callback JSON above
512 KiB before parsing.

Register and allow exactly `browser_sandbox_capture_text` for `main`; do not add
any matching capability for `browser-sandbox`.

- [ ] **Step 4: Run focused Rust tests**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test commands::browser -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test app_commands -- --nocapture'
```

Expected: all focused Browser tests pass.

- [ ] **Step 5: Commit capture command**

```bash
git add src-tauri/src/browser/state.rs src-tauri/src/commands/browser.rs src-tauri/src/app_commands.rs src-tauri/src/lib.rs src-tauri/capabilities/default.json
git commit -m "feat: capture fixed browser text evidence"
```

### Task 3: Exact prompt-context and session persistence integration

**Files:**
- Modify: `src-tauri/src/prompts/explicit_context.rs`
- Modify: `src-tauri/src/prompts/explicit_context_tests.rs`
- Modify: `src-tauri/src/prompts/assemble_tests.rs`
- Modify: `src-tauri/src/sessions/context_tests.rs`
- Modify: `src-tauri/src/sessions/validation.rs`
- Modify: `src/lib/api/chat.ts`
- Modify: `src/features/chat/contextSources.ts`
- Modify: `src/features/chat/contextSources.test.ts`
- Modify: `src/features/chat/ContextShelf.tsx`
- Modify: `src/features/chat/ContextShelf.test.tsx`
- Modify: `src/features/chat/contextDragPayload.ts`

**Interfaces:**
- Consumes: Task 1 `read_text_evidence`.
- Produces: `ContextSourceRef::BrowserTextEvidence { evidence_id }` and the exact `BrowserTextEvidence` manifest variant.

- [ ] **Step 1: Write failing resolver, manifest, session, and frontend identity tests**

Use this reference and manifest shape:

```rust
ContextSourceRef::BrowserTextEvidence { evidence_id: id.clone() }

ContextSourceManifestItem::BrowserTextEvidence {
    evidence_id: id,
    capture_kind: BrowserCaptureKind::Selection,
    source_url: "https://example.com/".into(),
    title: Some("Example".into()),
    captured_at_ms: 1,
    bytes: 13,
    redaction_count: 0,
    truncated: false,
}
```

Pin invalid id rejection, exact resolver content/manifest, missing/tampered store
blocking, no URL fetch, total-byte accounting, old shelf JSON compatibility,
project-session round-trip, local-session rejection, fork/rewind manifest
preservation, TypeScript normalization/key/label, and rejection from the private
drag MIME parser until drag support is intentionally added.

- [ ] **Step 2: Run focused tests and confirm failures**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test explicit_context -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test sessions::context_tests -- --nocapture'
npm test -- --run src/features/chat/contextSources.test.ts src/features/chat/ContextShelf.test.tsx src/features/chat/contextDragPayload.test.ts
```

Expected: missing union variants and unmatched switch failures.

- [ ] **Step 3: Implement the new typed source**

Add the Rust and TypeScript variants with identity `browser:<evidenceId>`.
`resolve_one` loads the immutable record from the trusted project and labels its
prompt block `Browser <selection|page> captured from <URL>`. Count `record.bytes`
in the existing 256 KiB explicit-context cap. Map manifests back to refs during
manifest validation. Keep the drag parser closed to this kind.

- [ ] **Step 4: Run focused backend and frontend tests**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test explicit_context -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test sessions::context_tests -- --nocapture'
npm test -- --run src/features/chat/contextSources.test.ts src/features/chat/ContextShelf.test.tsx src/features/chat/contextDragPayload.test.ts
npm run typecheck
```

Expected: all focused tests and typecheck pass.

- [ ] **Step 5: Commit context integration**

```bash
git add src-tauri/src/prompts src-tauri/src/sessions src/lib/api/chat.ts src/features/chat/contextSources.ts src/features/chat/contextSources.test.ts src/features/chat/ContextShelf.tsx src/features/chat/ContextShelf.test.tsx src/features/chat/contextDragPayload.ts
git commit -m "feat: resolve browser evidence in chat context"
```

### Task 4: Human capture actions and project-chat handoff

**Files:**
- Modify: `src/lib/api/browser.ts`
- Modify: `src/lib/api/browser.test.ts`
- Create: `src/features/browser/useBrowserEvidence.ts`
- Create: `src/features/browser/useBrowserEvidence.test.tsx`
- Modify: `src/features/browser/BrowserPanel.tsx`
- Modify: `src/features/browser/BrowserPanel.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/features/project-shell/UntrustedProjectView.tsx`
- Modify: `src/styles/layout/browser.css`

**Interfaces:**
- Consumes: Task 2 command and Task 3 `ContextSourceRef`.
- Produces: visible **Use selection in chat** / **Use page in chat** actions and the existing `AddContextSourceResult` handoff.

- [ ] **Step 1: Write failing API/hook/panel/shell tests**

Pin exact capture payload `{ captureKind: 'selection' | 'page' }`, stale response
suppression after unmount or a second capture, projectless/untrusted disabled
copy, loading/closed disabled state, empty-selection copy, visible redacted
preview/provenance, added/duplicate/full/unavailable outcomes, and trusted App
handoff opening project chat with shelf emphasis.

- [ ] **Step 2: Run focused tests and confirm failures**

```bash
npm test -- --run src/lib/api/browser.test.ts src/features/browser/useBrowserEvidence.test.tsx src/features/browser/BrowserPanel.test.tsx src/App.test.tsx
```

Expected: missing capture wrapper/hook/actions.

- [ ] **Step 3: Implement API, race-safe hook, and panel actions**

Add:

```ts
export type BrowserEvidenceSummary = {
  evidenceId: string;
  captureKind: 'selection' | 'page';
  sourceUrl: string;
  title: string | null;
  capturedAtMs: number;
  bytes: number;
  redactionCount: number;
  truncated: boolean;
  preview: string;
};

export function captureBrowserText(captureKind: 'selection' | 'page') {
  return invokeCommand<BrowserEvidenceSummary>('browser.sandboxCaptureText', { captureKind });
}
```

`useBrowserEvidence` owns one generation counter, `busy`, `summary`, and short
product copy. On success it constructs
`{ kind: 'browserTextEvidence', evidenceId }`, awaits `onUseInChat`, and keeps
the provenance card visible. `BrowserPanel` receives optional
`onUseInChat`; trusted `App` passes the existing handoff and the other shells
omit it. Do not add a second shelf or a new chat path.

- [ ] **Step 4: Run focused tests and typecheck**

```bash
npm test -- --run src/lib/api/browser.test.ts src/features/browser/useBrowserEvidence.test.tsx src/features/browser/BrowserPanel.test.tsx src/App.test.tsx
npm run typecheck
```

Expected: all focused tests and typecheck pass.

- [ ] **Step 5: Commit frontend capture UX**

```bash
git add src/lib/api/browser.ts src/lib/api/browser.test.ts src/features/browser src/App.tsx src/App.test.tsx src/features/project-shell/UntrustedProjectView.tsx src/styles/layout/browser.css
git commit -m "feat: add browser evidence to project chat"
```

### Task 5: Canonical docs, full verification, packaged smoke, and publication

**Files:**
- Modify: `docs/ROADMAP.md`
- Modify: `docs/IPC_CONTRACT.md`
- Modify: `docs/IPC_ROADMAP.md`
- Modify: `docs/SAFETY.md`
- Modify: `docs/AGENT_OPERABILITY.md`
- Modify: `docs/UI_STYLE.md`
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/SMOKE_TESTING.md`

**Interfaces:**
- Consumes: the verified behavior from Tasks 1-4.
- Produces: honest shipped-status docs and exact manual evidence.

- [ ] **Step 1: Update docs only after behavior is green**

Record page/selection capture, immutable project storage, redaction, caps,
exact-manifest semantics, project-only shelf placement, zero remote-page
authority, and no refetch. Correct `ROADMAP.md` so the human Browser workspace
is shipped. Keep screenshot evidence, drag/drop, agent actions, automatic
retrieval, and host control explicitly unshipped.

- [ ] **Step 2: Run focused and full verification**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test'
npm run typecheck
npm test
git diff --check
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
```

Expected: all Rust/frontend tests pass; verifier reports zero failures and only
the repository's existing documentation soft-cap warnings.

- [ ] **Step 3: Build and smoke the exact packaged app**

Use a disposable localhost page containing normal text, a selectable excerpt,
and one redactor-shaped test secret. Verify page capture, selection capture,
empty selection, redacted preview, projectless disabled copy, project chat
handoff, shelf persistence after relaunch, exact preview/send manifest, and no
new remote-page IPC. Delete the disposable fixture and stop its server/app.

- [ ] **Step 4: Commit docs and smoke evidence**

```bash
git add docs/ROADMAP.md docs/IPC_CONTRACT.md docs/IPC_ROADMAP.md docs/SAFETY.md docs/AGENT_OPERABILITY.md docs/UI_STYLE.md docs/FEATURE_INVENTORY.md docs/SMOKE_TESTING.md
git commit -m "docs: document browser text evidence"
```

- [ ] **Step 5: Publish and review**

Push `codex/browser-text-evidence`, open one focused PR, wait for GitHub verify
and gitleaks, dispatch a read-only exact-head reviewer, fix every genuine
Critical/Important finding, rerun the full gate, and squash-merge only with an
exact-head lock. Report the squash SHA, then continue the active goal into the
separate screenshot-evidence design.
