# Visible Context Drop Design

**Status:** approved for implementation. This is the drag-and-drop continuation of the shipped typed explicit-context shelf.

**Scope:** one frontend interaction milestone. A user can drag the current inspector file or selection, a Knowledge memory card, or a canonical Knowledge topic file into project chat through one visible destination. The interaction reuses the existing `ContextSourceRef`, `addContextSource`, session persistence, preview, send, and exact-manifest paths. It adds no backend prompt-content path, source kind, filesystem authority, or model-selected retrieval.

## Product outcome

Plume makes “put this into my next project-chat message” feel direct and obvious. While the user drags a valid source, a generous temporary destination appears in the current workspace view with the label **Drop into project chat**. Dropping there adds the exact opaque reference to the active project chat, opens that chat, and briefly emphasizes the canonical Context shelf item that was added.

The interaction follows a restrained, welcoming direction: one obvious action, generous spacing, soft motion, ordinary language, and no raw payloads or harness terminology. It should carry the clarity and small sense of delight of Codex's computer-use onboarding without copying its branding or adding permanent chrome. Think Apple restraint plus OpenAI approachability, expressed through Plume's existing paper-and-ink tokens.

Drag-and-drop is progressive enhancement. Existing **Use in chat** buttons remain the keyboard, screen-reader, touch, and precision-click path and must produce the same results.

## Layout constraint and chosen approach

Knowledge, Files, and Chat are mutually exclusive center views today. The canonical Context shelf therefore cannot be the literal drop target while the user is viewing a Knowledge card or inspector file.

Three approaches were considered:

1. **Temporary destination tray — chosen.** Show a large bottom destination only while a valid source is being dragged. It adds no idle-state clutter and states the result explicitly.
2. **Persistent mini shelf — rejected.** Keep a duplicate context strip above every workspace view. It is easy to target but duplicates the canonical shelf and conflicts with the consumer UI cleanup goal.
3. **Sidebar project-row target — rejected.** Reuse the project row as the drop target. It is compact but too small and too implicit for a general audience.

The temporary destination is an interaction bridge, not a second shelf. It never renders stored items, never owns state, and disappears when the drag ends.

## Supported drag sources

The first milestone supports only references the shelf already ships:

- a Knowledge memory card: `{ kind: 'memoryEntry', entryId }`;
- a canonical Knowledge topic card: `{ kind: 'topicFile', name }`;
- the current inspector file or non-empty selected line range: `{ kind: 'projectFile', relPath, startLine?, endLine? }`.

Core topic files, non-canonical topic paths, arbitrary filesystem rows, browser pages, screenshots, terminal output, clipboard text, models, and external desktop drops are out of scope. The current-file source is draggable only when the existing attach candidate is eligible. A selection snapshot is captured at drag start; later inspector changes do not rewrite the in-flight payload.

## Interaction states

### Source affordance

Supported cards and the eligible current-file control gain a quiet drag handle/cursor affordance and an accessible title such as **Drag to project chat**. The existing **Use in chat** control remains visible. Dragging text inside a memory or topic preview must continue to select text normally; drag begins from the dedicated handle or draggable action surface, not the entire article body.

### Destination

On a recognized Plume context drag, the current view reveals a fixed in-workspace destination above its bottom edge. It uses a soft tinted fill, a clear dashed outline, and the copy **Drop into project chat**. While the pointer is over it, the outline and fill strengthen slightly and the copy becomes **Release to add to chat**.

The tray does not appear for operating-system file drags, arbitrary text, URLs, unrecognized MIME types, malformed payloads, local-chat-only state, or while the active chat is streaming. It does not cover navigation, destructive controls, or the macOS title bar.

### Result

Dropping calls the same project-scope handoff used by **Use in chat**:

- `added`: open project chat, scroll/focus the canonical shelf, and softly emphasize the new row once;
- `duplicate`: open project chat and softly emphasize the existing matching row once;
- `full`: remain in the source view and show **Context is full. Remove an item in chat, then try again.**;
- `unavailable`: remain in the source view and show **Project chat is unavailable right now.**

No result silently discards a source. The tray closes after every drop or cancelled drag. The emphasis is presentation-only and does not change source order or persistence.

## Drag payload boundary

Add a focused frontend module that owns the custom drag MIME type, serialization, and parsing. The payload contains only one `ContextSourceRef`, never memory text, topic content, or file bytes. Parsing uses an explicit runtime type guard for the three shipped variants. Memory ids must match `m_` plus exactly 32 ASCII hexadecimal characters. Topic names must be `topics/<flat-name>.md`, with a non-empty non-dot-prefixed filename and no slash or backslash inside the filename. Project paths must be 1–1024 characters, project-relative, non-NUL, and contain no `..` path component. Optional line ranges must contain both endpoints as finite positive integers with `startLine <= endLine`.

The payload is not a security artifact. `addContextSource` still enforces identity and the 16-reference UX cap; session persistence and `chat.context`/`chat.send` continue to validate and resolve the reference at the backend trust boundary. A crafted browser drag cannot inject prompt content.

The custom MIME payload is read only from the drop event. No global drag registry or mutable singleton survives the gesture. A small React-owned boolean may control destination visibility, and drag-end/unmount cleanup must always clear it.

## Component boundaries

- `contextDragPayload.ts` owns the MIME constant, serializer, parser, and runtime guard.
- `ContextDragSource.tsx` provides the dedicated accessible drag handle/action wrapper used by Knowledge and the inspector attach surface.
- `ContextDropTray.tsx` owns drag-over depth, visible/hover states, drop parsing, and the result callback. It contains no session or navigation logic.
- `TrustedView` owns the cross-view handoff because it already owns `activeView`, `persisted.openScope('project')`, and `persisted.chat.addContextSource`.
- `ContextShelf` accepts an optional one-shot emphasis key and scroll/focus ref; it remains the only visible owner of the current shelf.

Knowledge cards and the inspector pass opaque refs into the shared drag source. They do not know how sessions are stored or how prompts are assembled.

## Accessibility and motion

- Every draggable source keeps a normal button with the same outcome.
- The drag handle has a descriptive accessible name; `draggable` alone is never the only instruction.
- The destination is supplementary and does not pretend to be keyboard-operable. Keyboard users activate **Use in chat**.
- Drop results are announced through one polite live region in the originating view.
- Shelf emphasis uses opacity/background/border transitions only, lasts no more than 900 ms, and is disabled under `prefers-reduced-motion: reduce`.
- No bouncing, glowing, confetti, sound, or repeated animation.

## Failure and race behavior

- The reference is snapshotted at drag start and parsed again at drop.
- A project close, scope switch, session switch, or stream start before drop yields `unavailable`; it never adds to a different scope.
- `openScope('project')` completes before `addContextSource`. If the session identity changes during that await, the handoff must re-check that it is still adding to the intended active project session or return `unavailable`.
- A duplicate preserves the first insertion position.
- A full shelf remains byte-for-byte unchanged.
- A source deleted after drop may initially add as a reference, but the existing shared preview marks it blocked and send re-resolution rejects it. Dragging does not bypass stale-source honesty.
- Drag leave uses depth accounting so movement across tray children does not flicker the hover state.
- Unmount and `dragend` always clear visibility and hover state.

## Testing

Focused frontend tests cover:

- serialize/parse round trips for all three reference variants;
- malformed JSON, foreign MIME, invalid kinds, invalid ranges, invalid memory ids, and non-canonical topic names are ignored;
- valid drag start exposes only the opaque ref and shows the tray;
- text selection on card content is not turned into a drag source;
- tray hover copy and non-flickering nested drag-leave behavior;
- `added`, `duplicate`, `full`, and `unavailable` outcomes;
- successful/duplicate drops navigate to project chat and emphasize the exact shelf key;
- full/unavailable drops remain in place with an announced message;
- streaming and local-chat boundaries hide or reject the destination;
- existing **Use in chat** buttons remain and share the same handoff;
- reduced-motion styling removes the emphasis transition.

Packaged UI smoke uses a trusted project and real pointer interaction: drag one memory and one topic from Knowledge into the temporary tray, confirm the project chat opens and the ordered shelf shows each once, drag the same memory again and confirm no duplicate, fill/reach the cap through the supported test setup and confirm the full result remains visible, then drag the inspector's current selection and confirm exact line provenance. Relaunch confirms the shelf persists through the already-shipped session path.

## Documentation honesty

Update `UI_STYLE.md`, `IPC_ROADMAP.md`, `SMOKE_TESTING.md`, and roadmap/status docs only after verification. State that drag-and-drop is a frontend placement gesture over the shipped typed shelf. It adds no semantic retrieval, no memory-link prompt authority, no browser evidence type, no external file import, and no computer-use capability.
