# Transcript-Native Research Artifacts

**Date:** 2026-07-20

**Status:** Approved product direction; awaiting written-spec review

**Base:** `codex/chat-context-cleanup@fb72d829569bdf960b18fdbe6c2c3859db004f9b`

## Goal

Make research feel like ordinary Plume chat. The user asks for research in the
composer, receives the answer as a normal assistant turn with clean source
links, and receives a Markdown attachment only after explicitly asking Plume
to export it.

The default chat surface must not show a research mode selector, artifact card,
preview/source tabs, Details disclosure, or permanent Export control.

## User experience

### Research

The user writes a normal message such as:

> Quickly research why some dinosaurs had feathers.

When the request can run against eligible evidence, Plume shows one quiet
progress line with Stop. Completion adds one normal assistant turn containing:

1. the readable research answer;
2. compact clickable source links at the end; and
3. a short review warning only when citation provenance needs review.

There is no separate research card or post-completion control strip. Exact
hashes, byte counts, model-call counts, and internal source ids do not appear
in the consumer transcript. They remain in the immutable backend artifact and
diagnostic evidence.

### Source links

Each visible source is rendered from the immutable source record, never from
arbitrary model-authored HTML. Clicking an HTTP(S) source asks the owning
chat's existing human-controlled Plume Browser to open that exact URL. The
normal Browser URL, origin, localhost, restoration, and ownership rules still
apply. A source click does not grant the model Browser authority.

If the Browser cannot open the source, Plume keeps the answer visible and
shows one small inline error beside the source link. It does not open an
external browser as a silent fallback.

### Explicit Markdown export

No file is created merely because research completed. Export begins only when
the user sends a normal chat message with an explicit Markdown export request,
for example:

> Export this as Markdown.

A narrow deterministic intent recognizer accepts only normalized requests in
these two forms, with an optional `please` prefix:

- `export this as markdown`; or
- `save this research as a markdown file`.

Matching is case-insensitive and ignores a final period. The recognizer
activates only when the owning session has a latest completed research
artifact. Every other message goes to normal chat and never writes a file.

The recognizer passes only the opaque owner, artifact id, and exact version to
Rust. Rust reuses the existing owned-artifact load and native save-panel export
path. The frontend never supplies or receives a filesystem path.

After a successful save, the transcript adds one assistant attachment:

> [dinosaur-research.md]

The attachment is backed by the immutable Plume artifact identity, not a raw
path. Clicking it offers the same exact version through the guarded native
export flow again. Plume does not claim that it can reopen or reveal the
previous destination because the current safety contract deliberately does
not retain that path. Native save-panel cancellation is quiet and adds no
attachment. A failed export adds one ordinary inline error and keeps the
explicit request available for retry.

## Honest capability boundary

This UI design does not turn Stage A into autonomous web research. The shipped
engine still accepts only eligible Browser text already attached to the owning
saved chat. It performs no search, URL fetch, Browser action, arbitrary tool,
shell command, patch, DOCX, or slide operation.

Until a separately reviewed search/fetch stage ships, a request without
eligible evidence receives one plain reply explaining that Plume needs source
text added from its Browser. The UI must not fabricate a web search or present
model memory as newly gathered sources.

The future demo target remains:

1. ask a research question in normal chat;
2. Plume performs separately shipped guarded search/fetch;
3. the answer contains clickable sources that open in Plume's Browser; and
4. an explicit later export request creates the Markdown attachment.

Steps 2 and any autonomous Browser navigation remain separate commissioned
capabilities rather than being smuggled into this presentation change.

## Architecture

### Transcript entry

Replace `ResearchArtifactCard` with a transcript-native research entry. The
entry uses the same visual shell, spacing, typography, copy affordance, and
scroll behavior as an ordinary completed assistant message.

The entry owns typed metadata rather than parsing its rendered Markdown:

```text
ResearchTranscriptEntry {
  owner,
  artifactId,
  version,
  citationStatus,
  markdown,
  sources[]
}
```

The answer body is rendered through the existing inert Markdown projection.
Only Rust-owned projected sources become interactive source actions. Model
HTML, images, arbitrary links, and scripts remain inert.

### Ordering and persistence

Research artifacts must appear in transcript chronology, not in a permanent
slot above the composer. The owning persisted session stores a typed assistant
artifact reference at completion, beside ordinary messages. On reload, Rust
re-resolves the reference through owner, artifact id, and version before the
frontend renders it. The stored chat record does not duplicate trusted source
bodies or gain authority from display text.

An export request is stored as the user's ordinary message. A successful
export response stores a typed attachment reference to the same artifact
version plus the safe display filename. It stores no destination path.

### Natural-language routing

Research request routing and export routing are narrow product intents, not a
broad tool executor. The first implementation removes the visible Create
selector. It starts bounded research only when the normalized message begins
with `research`, `please research`, or `quickly research`, and the owning saved
chat already has at least one eligible attached Browser-text source. The
remaining text is the research question. A matching request without eligible
evidence receives the plain source-needed reply and does not call the model.
The explicit Markdown-export forms are defined above. Every uncertain message
falls through to normal chat without taking an action.

This design does not expose `tools.invoke`, shell execution, model-authored
tool calls, or hidden automatic writes.

### Browser handoff

`ChatPanel` emits a typed source-open request containing the owning session and
exact sanitized HTTP(S) URL. Window-level routing activates that session's
Browser workspace and passes the URL through its existing explicit human
navigation path. Localhost continues to require its current exact-origin
approval. Non-HTTP(S), missing, or invalid source URLs render as plain source
labels rather than clickable controls.

## Removal scope

Remove from the consumer chat surface:

- the Create menu and research-mode selector;
- the research start-summary box;
- `ResearchArtifactCard` and its Open note, Sources, Details, and Export
  controls; and
- the post-completion research container above the composer.

Keep:

- the bounded research backend, events, Stop, citation checks, immutable
  bundles, owner gates, and native save panel;
- attached-source chips only while the user has explicitly attached context;
- exact provenance in backend evidence and automated tests; and
- ordinary visible errors when an explicitly requested action cannot finish.

## Testing

Implementation starts with failing tests proving:

1. completed research renders inside the transcript as an assistant message;
2. no research card controls or Create selector are present;
3. source actions use only immutable HTTP(S) source records and hand off to the
   owning Plume Browser;
4. completion alone never calls export;
5. an explicit Markdown export request exports the latest exact artifact;
6. ambiguous chat text never exports;
7. save cancellation adds no attachment and failure remains visible;
8. a successful save adds one typed Markdown attachment with no path;
9. reload preserves chronological artifact and attachment entries; and
10. all existing owner, citation, sequencing, cancellation, and redaction tests
    remain green.

The completion gate remains focused tests, full frontend tests, full Rust
verification, pre-commit and gitleaks, exact-head findings review, and packaged
1152x768 smoke. Packaged smoke must confirm that only chat, progress/Stop while
running, source links, and an explicitly requested Markdown attachment appear.

## Documentation and status

Update the user guide, smoke matrix, frontend domain map, feature inventory,
and current architecture/IPC contracts that own typed session messages and
Browser handoff. Keep `research.bounded-notes` partial until its remaining
packaged fault evidence is complete. Do not claim search, URL fetch, agent
Browser authority, or arbitrary tool execution.

PR #168 should be rewritten around this design before review. Its rejected
compact-card behavior must not remain as the final product surface.
