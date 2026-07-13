# Typed Explicit Context Shelf Design

**Status:** approved for implementation. This design specializes the already-approved context contract in `2026-07-12-roadmap-navigation-design.md` for the first shipped milestone.

**Scope:** one coherent milestone: typed per-session context references, manual **Use in chat** actions for the current file or selection, memory entries, and curated topic files, exact preview/send manifests, durable shelf state, and immutable per-turn provenance. Drag/drop is the next milestone on the same contract and is not part of this implementation.

## Product outcome

Project chat gains a visible **Context** shelf above the composer. A user can deliberately add:

- the current project file or selected line range;
- one exact memory entry;
- one curated `topics/<flat-name>.md` file.

The shelf is explicit, ordered, sticky, and scoped to one persisted project chat. It survives relaunch and session switching. It never appears in local chat, never crosses projects or sessions, and is never populated by memory-topic links. Links remain organization metadata only.

Each shelf item is a reference, not prompt text. The Rust backend resolves every reference again for preview and send through its owning store. A send is accepted only when every requested source is still valid. The synchronous `chat.send` response contains the exact ordered manifest that reached the bounded prompt, and that accepted manifest is copied onto the persisted user turn.

## Interaction model

### Adding sources

- The existing inspector action becomes **Use file in chat** or **Use selection in chat**.
- Each memory card in Knowledge gains **Use in chat**.
- Each canonical curated topic card in Knowledge gains **Use in chat**.
- Actions add to the active project session's shelf even when the central view is Knowledge rather than Chat, then navigate to Chat.
- Adding the same identity twice is idempotent. Identity is `(kind, stable id/path, line range)` and the first insertion position wins.
- A project shelf holds at most 16 references.

### Shelf behavior

- Items render in insertion order with kind, human label, and remove control.
- File items show `rel/path` or `rel/path:start-end`.
- Memory items show the stored preview but identify themselves by opaque memory id.
- Topic items show the canonical `topics/<name>.md` reference.
- Preview refreshes whenever the shelf, project, memory revision, or topic revision changes.
- Ready, checking, and blocked states remain visible. A blocked item carries a typed reason and is not silently removed.
- The shelf is sticky after a successful send. The user removes items deliberately.
- On a synchronous send rejection, the shelf remains byte-for-byte unchanged.

### Session behavior

- A shelf change is a stable persistence boundary, including on an otherwise-empty new chat.
- Selecting a session restores its own shelf.
- Local sessions reject non-empty shelves at the backend store boundary.
- Project switch resolution continues to come from backend scope routing; no command accepts a filesystem root.
- Continue/fork and rewind create a distinct child session with an empty current shelf. Historical user turns copied into the child preserve their accepted context manifests.
- Deleting a session deletes its shelf with the session row.

## Shared typed contract

TypeScript and Rust expose the same tagged reference enum:

```ts
export type ContextSourceRef =
  | {
      kind: 'projectFile';
      relPath: string;
      startLine?: number;
      endLine?: number;
    }
  | { kind: 'memoryEntry'; entryId: string }
  | { kind: 'topicFile'; name: string };
```

The resolved manifest is also tagged and ordered:

```ts
export type ContextSourceManifestItem =
  | {
      kind: 'projectFile';
      relPath: string;
      startLine: number | null;
      endLine: number | null;
      bytes: number;
      originalBytes: number;
      redactionCount: number;
    }
  | {
      kind: 'memoryEntry';
      entryId: string;
      createdAtMs: number;
      bytes: number;
      preview: string;
    }
  | {
      kind: 'topicFile';
      name: string;
      bytes: number;
    };
```

`bytes` is the UTF-8 byte count that reached the explicit-context prompt block for that item. For a project file it is post-redaction and post-line-slice. `originalBytes` retains the existing whole-file pre-redaction measurement. The memory preview is already-redacted stored text, collapsed to one line and Unicode-capped. Topic content never crosses IPC.

Preview returns one outcome per requested reference in request order:

```ts
export type ContextSourcePreviewItem =
  | { status: 'ready'; source: ContextSourceManifestItem }
  | {
      status: 'blocked';
      ref: ContextSourceRef;
      reason: ChatContextBlockReason;
      message: string;
    };
```

`chat.context` gains `contextSources?: ContextSourceRef[]` and returns `contextSources: ContextSourcePreviewItem[]`. `chat.send` gains the same request field and returns `contextSources: ContextSourceManifestItem[]` on acceptance.

The legacy singular `attachment` request remains accepted for wire compatibility. Supplying both `attachment` and `contextSources` is rejected as `BadArgument`. A legacy attachment is normalized to one `projectFile` reference and returned through both the existing attachment summary and the new manifest during the compatibility window. New frontend code sends only `contextSources`.

## Backend resolution and prompt assembly

Add a focused `prompts::explicit_context` module. It owns:

- reference shape validation and first-position deduplication;
- the 16-source cap;
- resolution in request order;
- the shared preview/send result;
- the total explicit-context budget;
- rendering one bounded system message after project instructions and before ambient memory/topics.

The total explicit-context content budget is 256 KiB. This preserves the old single-attachment ceiling while preventing a multi-source shelf from multiplying it. Each source also keeps its owning cap:

- project file: existing 256 KiB prompt-read cap, existing path, secret-name, binary, symlink, hardlink, and redaction controls;
- memory entry: exact opaque id from the current project memory store, existing 1 KiB stored-entry cap;
- topic file: exact canonical `topics/<flat-name>.md`, existing 8 KiB topic-file cap and symlink/hardlink safeguards.

Resolution is all-or-nothing for send. The resolver first evaluates every reference without mutating prompt messages. If any result is blocked, `chat.send` returns the first typed error and registers no stream. If every item is ready but their combined content exceeds 256 KiB, send rejects with `BadArgument`; it never truncates or silently drops a source. Preview reports every item independently and marks the first budget-overflowing item plus later items as blocked with the same stable `badArgument` category.

The rendered block uses explicit inert delimiters and labels each source as user-selected reference material, not instructions. It preserves shelf order. Memory and topic sources selected explicitly are separate from today's ambient bounded memory/core-topic context. To avoid duplicate prompt text, explicitly selected memory ids are excluded from ambient memory selection and explicitly selected topic names are not part of the ambient core trio anyway. Memory-topic links never participate in either selection path.

`assemble` returns the exact manifest produced by this module. `chat.context` calls the same resolver in preview mode. This shared ownership is the equality guarantee: ready preview manifests and accepted send manifests for unchanged storage are identical.

## Persistence

Session schema v4 adds two nullable JSON columns:

```sql
ALTER TABLE chat_sessions ADD COLUMN context_sources_json TEXT;
ALTER TABLE chat_messages ADD COLUMN context_manifest_json TEXT;
```

Fresh databases create both columns. The v3-to-v4 migration is atomic and leaves old rows `NULL`, which loads as an empty shelf or absent turn manifest.

`context_sources_json` stores the ordered `ContextSourceRef[]` for the current shelf. `context_manifest_json` stores the immutable accepted `ContextSourceManifestItem[]` only on user message rows. Both use normal serde tagged JSON and are validated after deserialization. Unknown kinds, malformed JSON, invalid paths/ids/names, over-cap arrays, manifests on assistant/cancelled/error rows, or non-empty project context in a local database make the store return `Corrupt`/`Invalid`; they are never silently discarded.

`sessions.saveTranscript` gains `contextSources?: ContextSourceRef[]` and atomically replaces the transcript plus current shelf in the same transaction. The response carries the stored shelf on `SessionRecord`. The frontend persistence bridge compares both transcript entry identity and shelf value, serializes shelf changes through its existing mutation queue, and restores both together on selection.

The visible `ChatEntry` user-message variant gains `contextSources?: ContextSourceManifestItem[]`. The legacy attachment fields remain readable for old rows and render as one provenance chip. New accepted sends persist only the new manifest.

To ensure "accepted" provenance rather than "attempted" provenance, a user entry with explicit sources is temporarily marked frontend-only as pending and excluded from persistence. When synchronous `chat.send` accepts, the returned manifest replaces the pending reference snapshot and the entry becomes persistable. When send rejects, the pending marker is removed without a manifest; the unchanged shelf shows the blocked source and the existing error row explains the rejected attempt.

## Frontend ownership

`useChat` owns the ordered shelf because it already survives central view changes and is hoisted by `usePersistedChat`. Its API gains:

- `contextSources`;
- `addContextSource(ref)` returning `added | duplicate | full | unavailable`;
- `removeContextSource(ref)`;
- `restore(entries, contextSources)`.

The API enforces identity deduplication and the 16-item UX cap, while the backend independently revalidates both session persistence and prompt resolution.

`usePersistedChat` exposes the hoisted shelf API through `chat`, observes shelf changes as save boundaries, and restores it from the session response. Async generation checks already used for session selection remain authoritative; a stale load must not overwrite the currently selected session's shelf.

`ContextShelf` and `ContextShelfItem` are focused chat siblings. `useChatContextPreview` accepts the full ordered reference list and exposes per-item outcomes. `ChatPanel` removes the one-shot `chip` state and existing `AttachBar` becomes an add-current-file control backed by `chat.addContextSource`.

The Knowledge workspace receives the same `ChatApi` plus a navigation callback. Its cards call `addContextSource` with opaque ids/canonical names, never text content. An add failure is visible inline and does not navigate away; success or duplicate navigates to Chat and focuses the shelf/composer.

## Failure and race behavior

- Preview generations invalidate on shelf mutation, project switch, unmount, memory revision, and topic revision. Late responses cannot replace newer state.
- Session loads restore transcript and shelf as one identity. A late old-session load cannot overwrite a newer selection.
- Send captures an immutable shelf snapshot. Shelf edits are disabled while streaming so the visible shelf cannot claim a different set than the in-flight turn.
- Backend resolution happens before stream registration. Any blocked/stale source produces no model call and no stream id registration.
- Memory deletion or topic deletion after preview but before send rejects the send; the shelf remains and the next preview marks the item blocked.
- A project change clears the old project surface through the existing session-scope transition. The backend still rejects any stale reference against the newly resolved project root.
- No frontend string is trusted as prompt content.

## Verification

Backend tests cover:

- exact ordered resolution for all three source kinds;
- deduplication and 16-item/256-KiB caps;
- file path, symlink, hardlink, secret-name, binary, line-range, and redaction behavior;
- exact memory-id lookup and stale deletion;
- strict topic naming, missing/oversize/symlink/hardlink behavior;
- preview/send manifest equality;
- all-or-nothing send before stream registration;
- links alone never select memory or topics;
- schema v3 migration, fresh v4, JSON corruption, local-scope rejection;
- shelf save/load/relaunch, session separation, deletion, project mismatch;
- fork/rewind empty current shelf plus preserved historical manifests.

Frontend tests cover:

- identity dedupe and insertion order;
- shelf persistence boundaries and stale-load protection;
- pending accepted provenance and rejection cleanup;
- ready/blocked preview rendering and independent retry;
- Use in chat from file selection, memory card, and topic card;
- local-chat absence and streaming mutation guard;
- exact turn chips after restore.

Packaged UI smoke covers a real trusted project: add a file selection, memory entry, and topic; confirm ordered shelf; remove one; relaunch and confirm restoration; send and confirm immutable turn provenance; delete a backing source and confirm blocked recovery without a model call. Drag/drop is explicitly deferred to the next milestone.

## Documentation honesty

Update `IPC_CONTRACT.md`, `PROMPT_CONTEXT.md`, `MEMORY.md`, `SESSIONS.md`, `SMOKE_TESTING.md`, `ROADMAP.md`, and the project status only after tests prove the behavior. State plainly that explicit context is manual reference placement, ambient memory/core topics still follow their existing bounded rules, links are metadata only, and no semantic retrieval, agent-chosen context, browser, or computer-use authority ships in this milestone.
