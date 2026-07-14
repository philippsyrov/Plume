# Browser Text Evidence Design

**Status:** Approved goal slice; implementation design for explicit Browser page and excerpt evidence.

## Goal

Let a human deliberately capture either the current page's visible text or the
current selection from Plume's sandboxed Browser, inspect the resulting
provenance, and place that immutable snapshot onto the existing project-chat
context shelf. The exact redacted snapshot that preview reports must be the
snapshot that send resolves later.

This is Browser evidence, not computer use. It adds no agent navigation,
automatic retrieval, hidden browsing, remote-page IPC authority, or macOS host
control.

## Chosen approach

The trusted `main` webview calls one fixed Rust command. Rust requires a trusted
project and a current, fully loaded Browser page, then evaluates a constant
read-only JavaScript observation through Tauri's `eval_with_callback`. The
script returns current URL, title, selected text, and a fixed-size prefix of
visible body text. No script string, selector, or expression crosses IPC.

Rust validates that the returned URL still matches the current Browser state,
applies byte caps and the existing secret redactor to title and content before
either reaches the frontend, and writes one immutable JSON evidence record under the trusted
project's `.plume/browser-evidence/` directory. The frontend receives metadata
plus an opaque `be_<32 hex>` reference, never the original pre-redaction text.

Two alternatives are rejected:

1. A narrow IPC bridge inside the remote page is simpler to message through,
   but it would let attacker-controlled page code call a Plume-owned channel and
   weaken the zero-capability proof.
2. Re-fetching the URL from Rust avoids DOM evaluation, but it would not capture
   the visible authenticated/session-specific page and would create hidden
   network activity unrelated to the user's open page.

## User surface

The Browser workspace adds two secondary actions while a page is open and not
loading:

- **Use selection in chat** captures the current non-empty selection.
- **Use page in chat** captures bounded visible body text.

On success Plume shows the captured kind, source host, byte count, redaction
count, truncation state, and a short redacted preview, then hands the opaque
reference to the same `addContextSource` path used by Files and Knowledge. The
project chat opens and emphasizes the canonical shelf chip. Duplicate and full
shelf outcomes reuse existing notices.

Without a trusted project the actions remain visible but disabled with the
plain-language hint **Open and trust a project to use Browser evidence in
chat.** An empty selection reports **Select text in the Browser window first.**
Capture failure never creates a shelf item.

No drag payload is added in this slice. Click placement proves the owning
resolver and manifest first; drag/drop can reuse the opaque reference later.

## Evidence store

Each JSON record contains:

- `version: 1`
- opaque evidence id
- `captureKind: "selection" | "page"`
- normalized source provenance URL with query/fragment removed and secret-shaped
  path content redacted
- optional bounded title
- capture timestamp
- redacted UTF-8 content
- stored byte count
- redaction count
- truncation flag

Caps are hard and deterministic:

- selection: 16 KiB stored content
- page: 64 KiB stored content
- title: 512 UTF-8 bytes
- callback payload: 512 KiB before parsing
- store: 100 records and 4 MiB total

The fixed script bounds the untrusted callback value before it crosses the
webview boundary; Rust then truncates the received prefix on a UTF-8 boundary
before redaction. The manifest says when either bound dropped content. The
store never silently evicts an older referenced record. Capacity exhaustion is
a typed, visible failure.

The writer refuses symlinked `.plume`, `browser-evidence`, or final-record paths,
writes through a sibling tempfile plus atomic rename, and never follows a
hardlink alias on prompt read. A project-scoped mutex serializes capacity check
and creation so concurrent captures cannot overrun the caps.

## Typed context contract

`ContextSourceRef` gains:

```text
{ kind: "browserTextEvidence", evidenceId: "be_..." }
```

Identity is the evidence id. Project sessions may persist the reference; local
sessions continue rejecting all context shelves. Preview and send resolve the
record from the currently trusted project, revalidate version/id/path/caps, and
produce this exact manifest:

```text
{
  kind: "browserTextEvidence",
  evidenceId,
  captureKind,
  sourceUrl,
  title,
  capturedAtMs,
  bytes,
  redactionCount,
  truncated
}
```

The prompt block labels the content as user-selected Browser reference material,
not instructions. Browser links are provenance only and never trigger a fetch.
Missing, malformed, symlinked, hardlinked, or oversized records are blocked in
preview and reject send. The accepted turn persists the exact manifest already
used by the existing session contract.

## Race and authority rules

- Capture is rejected while the Browser is loading or closed.
- Rust snapshots the Browser generation and expected URL before evaluation and
  rejects the callback if either changed before completion.
- The frontend generation-guards capture responses across project/view changes.
- The capture command is available only to `main`; `browser-sandbox` retains no
  Tauri application or core permission.
- The evaluated JavaScript is a Rust-owned constant. No arbitrary script,
  selector, or requested URL is accepted.
- Page content is untrusted reference material. Its title, provenance URL, and text cannot
  become instructions or authority.

## Screenshot boundary

Screenshot evidence is deliberately the next separate slice. A truthful
implementation needs a bounded WKWebView-native snapshot, app-owned image blob
storage, and either multimodal provider transport or local OCR with an exact
manifest. This slice does not pretend that a screenshot can reach today's
text-only prompt contract and does not use host screen-capture APIs.

## Verification

Backend tests pin fixed-script ownership, project trust, closed/loading/stale
page rejection, selection/page caps, UTF-8 truncation, redaction-before-IPC,
store capacity, atomic/symlink/hardlink defenses, old session compatibility,
preview/send exact-manifest parity, and local-session rejection.

Frontend tests pin disabled projectless copy, selection/page actions, empty
selection, duplicate/full/unavailable handoff, visible provenance, shelf labels,
session restoration, and stale async response guards. Full verification,
GitHub verify, gitleaks, exact-head review, and packaged smoke are required
before merge.
