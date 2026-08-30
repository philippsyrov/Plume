# Plume Feature Inventory

This is the only repository-wide implementation-status ledger. Domain docs
explain behavior; this file says whether the behavior is reachable.

- `shipped`: reachable production behavior with automated evidence.
- `partial`: useful end-to-end behavior exists, with a named missing capability.
- `scaffold`: types, pure logic, or UI shell exist without production execution.
- `researched`: adaptation is documented without shipped execution.
- `blocked`: accepted work waits on a named external dependency.
- `retired`: superseded behavior retained only for history.

Hardware evidence is independent from status. `hardware: pending` never means
the implementation is absent, and `shipped` never implies unrun hardware proof.

| Feature | Status | Current floor | Next honest step |
| --- | --- | --- | --- |
| Streaming chat | shipped | Ollama, Plume-managed MLX, and the host-gated Apple adapter stream cancellable token events into the chat UI. | Keep new provider adapters on the same event contract. |
| Apple Foundation Models bridge | shipped | The bundled helper, Rust adapter, and top-bar chooser route `apple-foundation/system` through the same prompt and terminal-event contract; actual availability remains host-reported. | Keep host availability and compatibility errors honest as Apple evolves the framework. |
| Session persistence | shipped | Local and trusted-project chats persist bounded transcripts and FTS search in separate SQLite stores. | Add cross-device sync only when commissioned. |
| Session branching | shipped | Users can continue or rewind into a new persisted chat with provenance. | Add branch comparison or merge only when commissioned. |
| Project trust and context | shipped | Persisted trust gates project instructions plus exact project file/selection, project-memory, topic, and Browser refs; app-private user-memory refs remain usable in local or project chat without gaining project authority. | Keep future source kinds behind their own bounded resolvers. |
| Exact context manifest | shipped | Sends, previews, and persisted user turns report the exact ordered explicit sources accepted by prompt assembly, including user/project memory and Browser provenance. | Preserve parity as future source kinds land. |
| Safe patch lifecycle | shipped | Validated diffs apply atomically through checkpoints and can be drift-checked and reverted. | Keep broader writes behind separate approval and allowlist gates. |
| User memory | shipped | App-private redacted **About you** entries support CRUD/search without a project and enter prompts only through explicit typed attachment. | Keep it non-ambient until retrieval earns separate approval. |
| Project memory entries | shipped | Trusted-project redacted entries support CRUD/search, bounded ambient context, and explicit exact attachment. | Keep project scope and prompt manifests exact. |
| Memory topics | shipped | Validated curated Markdown topics are browsable in Library and the core trio feeds bounded project prompt context. | Keep topic authority project-only. |
| Memory links | shipped | Library shows exact stored links/backlinks as organization metadata only. | Do not turn connections into retrieval authority. |
| Memory distillation | shipped | Users preview and apply exact-duplicate compaction with stale protection, link inheritance, and audit history. | Keep LLM-assisted summaries separate and opt-in. |
| Semantic memory retrieval | researched | Staged local semantic retrieval is documented. | Build an evaluation set after lexical preview and explicit insertion exist. |
| Project skill library | shipped | Trusted projects can list, inspect, preview, and explicitly write bounded skill files. | Add automatic improvement only behind reviewable drafts. |
| Session skill promotion | shipped | Selected project-chat messages become a redacted, snapshot-checked editable skill draft. | Preserve source provenance in later skill-improvement flows. |
| Bounded research notes | partial | Production UI/backend turn exact owner-shelf Browser text into an immutable cited Markdown note through Apple, fixed Qwen Coder, or fixed Qwen2-VL; Qwen2-VL may also inspect attached Browser screenshots. | Record the exact-head packaged Qwen2-VL screenshot/research/export matrix. |
| Agent single step | partial | A trusted MLX turn can validate a proposed diff and hand it to explicit patch apply/revert. | Connect the step to a bounded multi-iteration executor. |
| Bounded agent loop | scaffold | A tested pure controller models budget, pause, abort, failure, and completion. | Wire real model, read, patch, and approved command steps. |
| Tool catalog | scaffold | Read-only list/search exposes core and optional tool descriptions. | Put execution behind explicit approval and allowlist gates. |
| Plume-managed MLX | shipped | Releases bundle verified MLX-LM and MLX-VLM runtimes; fixed Qwen Coder and Qwen2-VL weights download explicitly and start app-wide, while arbitrary local folders retain the trusted-project path. | Keep runtime, weights, vision chat, and deeper agent claims separate. |
| Benchmark evidence | shipped | Deterministic harnesses, verified MLX/Plume paths, catalogs, presets, and a read-only viewer are reachable. | Run the full matrix on target hardware before D130 claims. |
| Library workspace | shipped | About you, This project, Topics, and exact Connections are scope-visible, independently loaded, searchable, and explicitly attachable by click/drag. | Keep retrieval automatic only after an evaluated preview milestone. |
| Conversation export | partial | One conversation renders to Markdown through the native Save panel, keeping cancelled turns, errors, and research-note bodies rather than dropping them. | Make a failed export visible to the user and offer it from the storage-cap notice. |
| Durable Home conversation | partial | Local chat opens into one backend-owned Home conversation in app-private storage, created idempotently and reachable through every existing session path. | Record the packaged relaunch smoke, then enforce the durable storage cap. |
| Session Browser foundation | shipped | Schema v5 and main-webview-only IPC persist bounded per-chat Browser layout, tabs, admitted history, restoration status, and app-private/project evidence in physically separate local/project stores. | Preserve the same ownership and privacy gates as Browser gains capabilities. |
| Browser workspace | shipped | Each persisted chat owns an integrated split/expanded WebKit Browser with visible navigation, restoration, fail-closed native-overlay recovery, exact-origin localhost approval, and explicit immutable evidence handoff. | Keep agent navigation authority behind the later guarded executor. |
| External computer operability | shipped | Labeled visible controls and status let external agents drive Plume through ordinary OS accessibility paths. | Keep new UI states accessible and recoverable. |
| Computer-use sandbox emission | researched | The capability-isolated human Browser exists, while agent approvals, target allowlist, trace, Pause/Stop, capture, and action execution remain research. | Add evidence first; require guarded execution gates before a bounded action. |
| Computer host control | researched | Separate opt-in macOS host-control gates are documented. | Revisit only after sandbox execution and safety evidence. |

```inventory-json
[
  {
    "id": "chat.streaming",
    "track": "local-chat",
    "status": "shipped",
    "currentBehavior": "Ollama, Plume-managed MLX, and Apple On-Device stream cancellable token events into the chat UI. Ollama and MLX read through the shared 1 MiB bounded line reader; fixed catalog Qwen Coder also sends its reviewed ChatML stop string so the control marker is not rendered. Fixed Qwen2-VL accepts exact Rust-resolved PNG attachments through MLX-VLM. Apple uses a separately bounded JSON-lines helper channel and the same sequenced token/done contract.",
    "missingBehavior": "Additional provider adapters must still adopt the same streaming event contract; provider chat does not itself supply a multi-step coding-agent executor.",
    "frontendReachability": "Chat workspace composer, transcript, streaming cursor, and Stop control.",
    "backendReachability": "chat.send and chat.cancel dispatch through Ollama, the exact-handle MLX-LM/MLX-VLM adapter, or exactly apple-foundation/system with no handle.",
    "automatedEvidence": [
      "src-tauri/src/commands/chat/send_tests.rs",
      "src-tauri/src/chat/mlx_lm_tests.rs",
      "src-tauri/src/chat/ollama/streaming_tests.rs",
      "src-tauri/src/chat/apple_foundation_tests.rs",
      "src/features/chat/ChatPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "hardware: packaged Calm UI implementation head 4a4e329a5e33bf2103b3f372b9d7a7a70aa8ecc0 selected available Apple On-Device and returned the exact requested `Calm.` reply in 2.0 s, then started the already-installed fixed Qwen and returned the exact requested `Qwen calm.` reply in 646 ms / 5 tokens with no control marker. The transcript kept quiet You/Plume labels plus model and duration evidence.",
    "dependencies": ["selected reachable model", "Ollama compatibility runtime, Plume-managed MLX handle, or available Apple system model"],
    "implementationPaths": [
      "src-tauri/src/commands/chat/send.rs",
      "src-tauri/src/chat/mlx_lm.rs",
      "src-tauri/src/chat/stream_read.rs",
      "src-tauri/src/chat/apple_foundation.rs",
      "src/features/chat/useChat.ts"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/MODEL_PROVIDERS.md"],
    "nextCommissionedSlice": "Keep new provider adapters on the same event contract",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "providers.apple-foundation",
    "track": "local-models",
    "status": "shipped",
    "currentBehavior": "The top-bar catalog can select Apple On-Device before a project is open when the host reports SystemLanguageModel.default available. Rust preserves the normal prompt assembly, trust, redaction, exact-manifest, cancellation, sequencing, and persistence path, then launches one bounded bundled helper with no server handle, localhost port, project path, tool interface, or Qwen fallback.",
    "missingBehavior": "Availability is not universal: unsupported OS, device eligibility, Apple Intelligence state, model readiness, and generation success remain host/framework facts. No Private Cloud Compute, Apple tool calling, or image input is claimed here.",
    "frontendReachability": "One top-bar Model control opens a compact inline Models workspace rather than covering the active task; unavailable hosts keep a short disabled reason and optional details.",
    "backendReachability": "providers.appleAvailability and chat.send for exactly apple-foundation/system with no handleId.",
    "automatedEvidence": [
      "src-tauri/apple-model/Tests/PlumeAppleModelTests/GenerationTests.swift",
      "src-tauri/src/providers/apple_foundation_tests.rs",
      "src-tauri/src/chat/apple_foundation_tests.rs",
      "src/features/model-picker/ModelChooser.test.tsx",
      "src/features/model-picker/useModelCatalog.test.tsx",
      "src/features/chat/disabledReason.test.ts"
    ],
    "manualOrHardwareEvidence": "hardware: packaged Calm UI implementation head 4a4e329a5e33bf2103b3f372b9d7a7a70aa8ecc0 verified host availability, compact-row selection, and the exact requested `Calm.` reply in 2.0 s. Packaged final-review implementation head 2b42926fbeb4cce0f7540fd0e1f8f50c6c2fc0a8 reselected Apple from the compact row through Computer Use; that smoke did not repeat generation. Packaged inline-workspace implementation head ff2576a6005da7699e0ad4a77b7426c3049b23f9 verified Apple remained available in the inline Models view with no artifact overlay; that smoke did not repeat selection or generation. Cancellation remains exercised on ancestor package 7e7b44df98cb0b3c3b966cd19d6fc3410b1c8409.",
    "dependencies": ["macOS 26 or newer", "eligible Apple Silicon host", "Apple Intelligence and system model ready", "bundled Apple helper"],
    "implementationPaths": [
      "src-tauri/apple-model/Sources/PlumeAppleModel/",
      "src-tauri/src/providers/apple_foundation.rs",
      "src-tauri/src/chat/apple_foundation.rs",
      "src/features/model-picker/useModelCatalog.ts",
      "src/features/model-picker/ModelChooser.tsx"
    ],
    "sourceDocuments": ["docs/MODEL_PROVIDERS.md", "docs/IPC_CONTRACT.md", "docs/SAFETY.md"],
    "nextCommissionedSlice": "Keep Apple provider additions availability-gated and evidence-led without broadening provider authority",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "sessions.persistence",
    "track": "sessions",
    "status": "shipped",
    "currentBehavior": "Local and trusted-project chats persist bounded transcripts and FTS search in separate SQLite stores. Active rows stay in their scoped sidebar sections; archived local and project chats are managed together under separate Settings sections.",
    "missingBehavior": "No cross-device sync is shipped. Conversation export ships as its own record; this one covers the store.",
    "frontendReachability": "Scoped session sidebar, Settings Archived sections, search overlay, and stable-boundary transcript saves.",
    "backendReachability": "All thirteen registered verbs: sessions.list, create, home, storage, load, fork, rollback, rename, archive, delete, export, saveTranscript, and search.",
    "automatedEvidence": [
      "src-tauri/src/sessions/tests.rs",
      "src/features/sessions/usePersistedChat.test.tsx",
      "src/features/sessions/SessionDialogs.test.tsx",
      "src/features/project-shell/UnifiedChrome.test.tsx"
    ],
    "manualOrHardwareEvidence": "Packaged shell-cleanup implementation head 9243b504640087f308112a5b7ed0c9045ef97dbe opened Settings Archived through ordinary OS accessibility and showed archived local rows with Unarchive and More controls; local/project separation and streaming-delete guards remain exact component-test evidence.",
    "dependencies": ["app-data directory for local chats", "trusted project for project chats"],
    "implementationPaths": [
      "src-tauri/src/sessions/mod.rs",
      "src-tauri/src/sessions/schema.rs",
      "src-tauri/src/sessions/transcript.rs",
      "src-tauri/src/sessions/home.rs",
      "src-tauri/src/sessions/storage.rs",
      "src-tauri/src/sessions/export.rs",
      "src-tauri/src/commands/sessions.rs",
      "src/features/sessions/usePersistedChat.ts",
      "src/features/sessions/SessionDialogs.tsx",
      "src/features/project-shell/UnifiedChrome.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/AGENT_OPERABILITY.md"],
    "nextCommissionedSlice": "No sync slice commissioned",
    "lastVerifiedCommit": "cbbbc28af7005e30af4bcf34f315a20474d7b422",
    "lastVerifiedDate": "2026-08-30"
  },
  {
    "id": "sessions.branching",
    "track": "sessions",
    "status": "shipped",
    "currentBehavior": "Users can continue a full chat or rewind selected turns into a new persisted chat with parent and boundary provenance.",
    "missingBehavior": "Branches cannot be compared or merged back together.",
    "frontendReachability": "Compact Continue and Rewind actions on persisted local or project session rows, with their safety explanation behind an inline disclosure.",
    "backendReachability": "sessions.fork and sessions.rollback perform atomic transcript branches.",
    "automatedEvidence": [
      "src-tauri/src/sessions/fork_tests.rs",
      "src-tauri/src/sessions/rollback_tests.rs",
      "src/features/sessions/usePersistedChat.test.tsx",
      "src/features/sessions/SessionRow.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["persisted project session", "trusted project"],
    "implementationPaths": [
      "src-tauri/src/sessions/branch.rs",
      "src-tauri/src/sessions/mod.rs",
      "src/features/sessions/usePersistedChat.ts",
      "src/features/sessions/SessionRow.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/AGENT_OPERABILITY.md"],
    "nextCommissionedSlice": "No branch comparison or merge slice commissioned",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "project.trust-and-context",
    "track": "project-context",
    "status": "shipped",
    "currentBehavior": "Native macOS selection, Finder folder drop, or disclosed manual entry produce a candidate path that still passes through project.open validation and explicit trust review. Persisted project trust gates project instructions plus exact project-file or selection, project-memory, curated-topic, and project Browser refs; app-private user-memory and owned local Browser refs remain usable without project authority. Files and Library expose typed click/drag placement, while Browser exposes explicit human capture buttons over the same shelf. Pinned shelf sources stay distinct from visible bounded ambient project instructions, memory, and topics.",
    "missingBehavior": "Automatic retrieval authority and agent-driven browser actions are not shipped.",
    "frontendReachability": "Open project offers native choose, visible Finder drop, and disclosed manual path entry; local/project chat context shelves expose click-or-drag Use in chat controls in Files and Library plus explicit Browser selection/page-text/screenshot capture.",
    "backendReachability": "chat.context and chat.send resolve typed refs through their owning trusted bounded readers before any stream registration.",
    "automatedEvidence": [
      "src-tauri/src/project/trust.rs",
      "src-tauri/src/prompts/assemble_tests.rs",
      "src-tauri/src/prompts/explicit_context_tests.rs",
      "src-tauri/src/browser/evidence_tests.rs",
      "src-tauri/src/browser/screenshot_evidence_tests.rs",
      "src/features/chat/ChatPanel.test.tsx",
      "src/features/chat/ContextShelf.test.tsx",
      "src/features/chat/ContextDropSurface.test.tsx",
      "src/features/chat/contextDragPayload.test.ts",
      "src/features/chat/useChat.test.tsx",
      "src/features/project-shell/OpenProjectModal.test.tsx",
      "src/features/browser/BrowserPanel.test.tsx",
      "src/features/sessions/usePersistedChat.test.tsx"
    ],
    "manualOrHardwareEvidence": "Packaged Build Week candidate smoke at 2a3520e verified the File/Web/Memory shelf in wide Chat and narrow Browser split, plus Files, Browser, and Library handoff into the same persisted trusted-project chat. Packaged inline-workspace implementation head ff2576a6005da7699e0ad4a77b7426c3049b23f9 verified Open Project as inline workspace content, the native macOS directory panel, cancellation back to the same view, and disclosed manual entry; no project was opened during that smoke. Packaged compact-chat implementation head 4b73f06a0f0da752fec43f81188847c2740860d5 verified the attached Browser source as one concise chip with exact provenance retained under Details.",
    "dependencies": ["owning persisted session", "trusted project for project-only source kinds"],
    "implementationPaths": [
      "src-tauri/src/commands/project.rs",
      "src-tauri/src/project/trust.rs",
      "src-tauri/src/prompts/assemble.rs",
      "src-tauri/src/prompts/explicit_context.rs",
      "src-tauri/src/browser/evidence.rs",
      "src-tauri/src/browser/screenshot_evidence.rs",
      "src/features/project-shell/UnifiedChrome.tsx",
      "src/features/project-shell/useProjectFolderDrop.ts",
      "src/features/chat/ContextShelf.tsx",
      "src/features/chat/ContextDropSurface.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/SAFETY.md"],
    "nextCommissionedSlice": "No automatic retrieval or agent browser action slice commissioned",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "context.exact-manifest",
    "track": "project-context",
    "status": "shipped",
    "currentBehavior": "Chat preview, send acceptance, and persisted user turns carry the exact ordered project-file, project-memory, user-memory, topic-file, Browser-text, and Browser-screenshot sources accepted by bounded prompt assembly. Apple, MLX, and Ollama dispatch only after that same manifest and redaction path succeeds.",
    "missingBehavior": "Future source kinds are not accepted until their owning resolver and manifest ship.",
    "frontendReachability": "Compact per-source shelf chips keep exact readiness and removable attachment state; immutable accepted-context chips remain on user turns, with provenance available under Details.",
    "backendReachability": "chat.context resolves per-source outcomes and chat.send returns the accepted explicit manifest before the user turn becomes persistable.",
    "automatedEvidence": [
      "src-tauri/src/prompts/assemble_tests.rs",
      "src-tauri/src/commands/chat/send_tests.rs",
      "src-tauri/src/chat/apple_foundation_tests.rs",
      "src-tauri/src/prompts/explicit_context_tests.rs",
      "src/features/chat/ContextShelf.test.tsx",
      "src/features/chat/useChat.test.tsx",
      "src/features/sessions/usePersistedChat.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["owning persisted session", "bounded prompt assembly", "trusted project for project-only refs"],
    "implementationPaths": [
      "src-tauri/src/prompts/explicit_context.rs",
      "src-tauri/src/commands/chat/context.rs",
      "src-tauri/src/commands/chat/send.rs",
      "src/features/chat/ContextShelf.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"],
    "nextCommissionedSlice": "Preserve exact preview/send/persistence parity for every future source kind",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "chat.conversation-export",
    "track": "conversation",
    "status": "partial",
    "currentBehavior": "sessions.export renders one conversation to Markdown and offers it through the native Save panel; cancelling is an ordinary outcome and no path is ever accepted from or returned to the frontend. The rendering keeps what an export could most easily misrepresent: a cancelled turn keeps the partial answer the user saw, an error turn appears as itself, and a research entry carries the note body resolved from the artifact store rather than a placeholder. Transcript prose is escaped only where it could restructure the document from column zero \u2014 ATX headings and thematic breaks \u2014 and text placed inside the export's own emphasis or heading markers is escaped so it cannot close them early.",
    "missingBehavior": "A failed export is written to the console and never shown: src/features/sessions/SessionDialogs.tsx catches the rejection without surfacing it, so a refused path or a write failure looks to the user like a cancelled save. Export is also not yet offered from the storage-cap notice, which is the recovery it exists to be. Both are addressed by PR #188, which is open and not merged. No packaged export smoke is recorded.",
    "frontendReachability": "Export in the session row menu, in both the project shell and the projectless shell.",
    "backendReachability": "sessions.export takes { scope, sessionId } and returns { status: 'cancelled' } or { status: 'saved', fileName }.",
    "automatedEvidence": [
      "src-tauri/src/sessions/export_tests.rs",
      "src/features/sessions/SessionDialogs.test.tsx"
    ],
    "manualOrHardwareEvidence": "packaged export smoke pending",
    "dependencies": [
      "session persistence",
      "research artifact store",
      "native save panel"
    ],
    "implementationPaths": [
      "src-tauri/src/sessions/export.rs",
      "src-tauri/src/commands/sessions.rs",
      "src/lib/api/sessions.ts",
      "src/features/sessions/SessionDialogs.tsx"
    ],
    "sourceDocuments": [
      "docs/IPC_CONTRACT.md",
      "docs/superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md"
    ],
    "nextCommissionedSlice": "Visible export failure and the storage-cap recovery link (PR #188)",
    "lastVerifiedCommit": "cbbbc28af7005e30af4bcf34f315a20474d7b422",
    "lastVerifiedDate": "2026-08-30"
  },
  {
    "id": "chat.home-conversation",
    "track": "conversation",
    "status": "partial",
    "currentBehavior": "Local chat opens into one backend-owned Home conversation in app-private storage. Schema v7 marks it with is_home behind a partial unique index, sessions.home creates it idempotently, and startup resolves it from the backend rather than selecting the most recently updated chat.",
    "missingBehavior": "The packaged relaunch smoke is not recorded. Every consumer that needs a local session still resolves it its own way: src/features/library/libraryChatHandoff.ts and src/App.tsx call startNewSession, so a Library or Browser handoff on a session-less local surface creates an ordinary chat beside Home instead of using it. Addressed by PR #190, which is open and not merged.",
    "frontendReachability": "Startup with no open project lands in Home. The Browser and Library still mint an ordinary chat when the local surface has none, rather than attaching to Home.",
    "backendReachability": "sessions.home takes an empty payload and is local scope only; the frontend never supplies the Home id.",
    "automatedEvidence": [
      "src-tauri/src/sessions/home_tests.rs",
      "src/features/sessions/usePersistedChat.test.tsx"
    ],
    "manualOrHardwareEvidence": "packaged relaunch smoke pending",
    "dependencies": ["app-private session store"],
    "implementationPaths": [
      "src-tauri/src/sessions/home.rs",
      "src-tauri/src/sessions/schema.rs",
      "src-tauri/src/commands/sessions.rs",
      "src/features/sessions/usePersistedChat.ts"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/ARCHITECTURE.md", "docs/superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md"],
    "nextCommissionedSlice": "One shared Home resolution for every consumer (PR #190), packaged relaunch smoke, then Phase 2 transparent compaction",
    "lastVerifiedCommit": "cbbbc28af7005e30af4bcf34f315a20474d7b422",
    "lastVerifiedDate": "2026-08-30"
  },
  {
    "id": "patch.safe-lifecycle",
    "track": "safe-editing",
    "status": "shipped",
    "currentBehavior": "Unified diffs are path-validated, pre-image checked, checkpointed, atomically applied, drift-checked, and revertible.",
    "missingBehavior": "Arbitrary filesystem writes and shell execution remain outside this patch-only lifecycle.",
    "frontendReachability": "Validated diff cards expose explicit Apply and Revert controls in chat and single-step runs.",
    "backendReachability": "patch.validate, patch.apply, and patch.revert operate only inside the trusted project.",
    "automatedEvidence": [
      "src-tauri/src/patch/apply_tests.rs",
      "src-tauri/src/patch/revert_tests.rs",
      "src/features/agent/AgentSingleStepPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["trusted project", "valid unified diff", "explicit user apply or revert"],
    "implementationPaths": [
      "src-tauri/src/patch/validate.rs",
      "src-tauri/src/patch/apply.rs",
      "src-tauri/src/patch/revert.rs"
    ],
    "sourceDocuments": ["docs/SAFETY.md", "docs/IPC_CONTRACT.md"],
    "nextCommissionedSlice": "Keep broader writes behind separate approval and allowlist gates",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "memory.user-entries",
    "track": "library",
    "status": "shipped",
    "currentBehavior": "App-private About you entries support bounded redacted CRUD and lexical search without a project, and enter local or project prompts only through an explicit userMemoryEntry ref.",
    "missingBehavior": "User memory has no ambient injection, topic links, semantic retrieval, automatic contradiction handling, distillation, or background dreaming.",
    "frontendReachability": "Library About you browse/search/detail and Settings Library About you create/edit/forget controls; explicit Use in chat click/drag on eligible rows.",
    "backendReachability": "memory.userIndex, userRemember, userUpdate, userForget, and userSearch resolve only the backend-owned app-data store; chat.context/send resolve explicit userMemoryEntry refs from that store.",
    "automatedEvidence": [
      "src-tauri/src/memory/user_store_tests.rs",
      "src-tauri/src/prompts/explicit_context_tests.rs",
      "src-tauri/src/commands/chat/context_tests.rs",
      "src-tauri/src/commands/chat/send_tests.rs",
      "src-tauri/src/sessions/context_tests.rs",
      "src/features/library/useLibraryData.test.tsx",
      "src/features/library/LibraryPanel.test.tsx",
      "src/features/library/LibrarySettingsPanel.test.tsx",
      "src/features/chat/useChat.test.tsx"
    ],
    "manualOrHardwareEvidence": "Packaged-app Library smoke covers projectless About you CRUD plus exact local/project attachment; no model or special hardware is required until the send step.",
    "dependencies": ["Tauri app-data directory", "persisted chat session for explicit attachment"],
    "implementationPaths": [
      "src-tauri/src/memory/user_store.rs",
      "src-tauri/src/commands/memory.rs",
      "src-tauri/src/prompts/explicit_context.rs",
      "src/features/library/useLibraryData.ts",
      "src/features/library/LibrarySettingsPanel.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/ROADMAP.md"],
    "nextCommissionedSlice": "No automatic user-memory retrieval slice commissioned",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "memory.entries",
    "track": "project-knowledge",
    "status": "shipped",
    "currentBehavior": "Users can create, read, update, forget, text-search, and prompt-inject bounded redacted project memory entries.",
    "missingBehavior": "Entries have no semantic retrieval, automatic contradiction handling, or background dreaming.",
    "frontendReachability": "Settings Library This project controls plus Library This project browse/search/detail and explicit Use in chat.",
    "backendReachability": "memory.index, remember, update, forget, and search over the trusted JSONL store.",
    "automatedEvidence": [
      "src-tauri/src/memory/memory_tests.rs",
      "src/features/memory/MemoryPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["trusted project"],
    "implementationPaths": [
      "src-tauri/src/memory/mod.rs",
      "src-tauri/src/memory/store.rs",
      "src/features/memory/MemoryPanel.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/LOCAL_AGENT_NORTH_STAR.md"],
    "nextCommissionedSlice": "No automatic project-memory retrieval slice commissioned",
    "lastVerifiedCommit": "f4138ae57a908463b746ca20d04490a8274d1092",
    "lastVerifiedDate": "2026-07-15"
  },
  {
    "id": "memory.topics",
    "track": "project-knowledge",
    "status": "shipped",
    "currentBehavior": "Users can browse validated curated Markdown topics, while non-empty INDEX.md, USER.md, and SOUL.md feed bounded prompt context.",
    "missingBehavior": "Topics are not generated automatically and do not authorize semantic retrieval.",
    "frontendReachability": "Library Topics navigation/detail plus Settings Library This project topic controls and chat Topics badge.",
    "backendReachability": "memory.topics reads the curated files and prompt assembly consumes the capped core trio.",
    "automatedEvidence": [
      "src-tauri/src/memory/memory_tests.rs",
      "src/features/memory/MemoryTopics.test.tsx",
      "src-tauri/src/prompts/assemble_tests.rs"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["trusted project", "curated Markdown files under .plume/memory"],
    "implementationPaths": [
      "src-tauri/src/memory/topics.rs",
      "src/features/memory/MemoryTopics.tsx",
      "src-tauri/src/prompts/assemble.rs"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/LOCAL_AGENT_NORTH_STAR.md"],
    "nextCommissionedSlice": "No automatic topic-generation slice commissioned",
    "lastVerifiedCommit": "f4138ae57a908463b746ca20d04490a8274d1092",
    "lastVerifiedDate": "2026-07-15"
  },
  {
    "id": "memory.links",
    "track": "project-knowledge",
    "status": "shipped",
    "currentBehavior": "Users link remembered entries to validated curated topic files.",
    "missingBehavior": "Links do not select prompt context or semantic retrieval.",
    "frontendReachability": "Settings Library project link editor plus Library Connections and exact backlink detail.",
    "backendReachability": "memory.setLinks over the trusted project store.",
    "automatedEvidence": [
      "src-tauri/src/memory/memory_tests.rs",
      "src/features/memory/MemoryPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["trusted project", "curated topic file"],
    "implementationPaths": [
      "src-tauri/src/memory/links.rs",
      "src/features/memory/MemoryPanel.tsx"
    ],
    "sourceDocuments": [
      "docs/IPC_CONTRACT.md",
      "docs/MEMORY_DISTILLATION.md"
    ],
    "nextCommissionedSlice": "No link-driven retrieval slice commissioned",
    "lastVerifiedCommit": "f4138ae57a908463b746ca20d04490a8274d1092",
    "lastVerifiedDate": "2026-07-15"
  },
  {
    "id": "memory.distillation",
    "track": "project-knowledge",
    "status": "shipped",
    "currentBehavior": "Users preview and apply exact-normalized duplicate compaction with state-bound ids, link inheritance, conflicts, and a visible audit log.",
    "missingBehavior": "LLM-assisted clustering, summaries, contradiction pruning, scheduling, and undo are not shipped.",
    "frontendReachability": "Memory settings Find duplicates disclosure, group selector, Compact action, and recent compactions.",
    "backendReachability": "memory.distillPreview, memory.distillApply, and memory.distillLog over the trusted store.",
    "automatedEvidence": [
      "src-tauri/src/memory/memory_tests.rs",
      "src/features/memory/MemoryDistill.test.tsx",
      "src/features/memory/MemoryPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["trusted project", "user-confirmed duplicate groups"],
    "implementationPaths": [
      "src-tauri/src/memory/distill.rs",
      "src/features/memory/MemoryDistill.tsx"
    ],
    "sourceDocuments": ["docs/MEMORY_DISTILLATION.md", "docs/IPC_CONTRACT.md"],
    "nextCommissionedSlice": "No LLM-assisted distillation slice commissioned",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "memory.semantic-retrieval",
    "track": "project-knowledge",
    "status": "researched",
    "currentBehavior": "A staged path from lexical preview and explicit insertion to measured local semantic retrieval is documented.",
    "missingBehavior": "No embeddings, vector index, semantic ranker, retrieval preview, or automatic semantic prompt insertion exists.",
    "frontendReachability": "None.",
    "backendReachability": "None.",
    "automatedEvidence": [],
    "manualOrHardwareEvidence": "research only",
    "dependencies": ["explicit context shelf", "retrieval evaluation set", "local embedding decision"],
    "implementationPaths": [],
    "sourceDocuments": [
      "docs/LOCAL_AGENT_NORTH_STAR.md",
      "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"
    ],
    "nextCommissionedSlice": "Build lexical preview and explicit insertion before semantic retrieval",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "skills.project-library",
    "track": "skills-tools",
    "status": "shipped",
    "currentBehavior": "Trusted projects can list, load, preview, and explicitly write bounded validated Markdown skills.",
    "missingBehavior": "Skills are not automatically created, improved, or executed as hidden authority.",
    "frontendReachability": "Project Settings Skills library and explicit preview/apply editor.",
    "backendReachability": "skills.list, skills.load, skills.preview, and skills.apply within the trusted project.",
    "automatedEvidence": [
      "src-tauri/src/skills/tests.rs",
      "src/features/skills/SkillsPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["trusted project", "valid project skill path and metadata"],
    "implementationPaths": [
      "src-tauri/src/skills/store.rs",
      "src-tauri/src/commands/skills.rs",
      "src/features/skills/SkillsPanel.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/LOCAL_AGENT_NORTH_STAR.md"],
    "nextCommissionedSlice": "Keep automatic improvement behind reviewable drafts",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "skills.session-promotion",
    "track": "skills-tools",
    "status": "shipped",
    "currentBehavior": "Users select bounded project-chat messages and produce a redacted, snapshot-checked editable skill draft before explicit preview and apply.",
    "missingBehavior": "Promotion does not automatically write, run, or improve the skill.",
    "frontendReachability": "Skills settings Promote from project chat disclosure and draft editor.",
    "backendReachability": "skills.promotionContext and skills.promotePreview validate session scope, selection, snapshot, and redaction.",
    "automatedEvidence": [
      "src-tauri/src/skills/tests.rs",
      "src/features/skills/SkillsPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["trusted project", "persisted project session", "selected transcript entries"],
    "implementationPaths": [
      "src-tauri/src/skills/promotion.rs",
      "src-tauri/src/commands/skills.rs",
      "src/features/skills/ChatSkillDraft.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"],
    "nextCommissionedSlice": "Preserve source provenance in later skill-improvement flows",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "research.bounded-notes",
    "track": "knowledge-artifacts",
    "status": "partial",
    "currentBehavior": "The production Chat composer recognizes a narrow Research prompt and can start a bounded Stage A note from exact Browser text already attached to the owning local or project session. Rust re-resolves those immutable captures and orchestrates Apple On-Device, fixed Qwen Coder, or fixed Qwen2-VL through summary/draft framing. Qwen2-VL research may additionally receive exact owner-shelf Browser screenshot PNGs; the immutable artifact retains their separate evidence id, URL/title, capture time, SHA-256, dimensions, and byte-count provenance, while text remains required and is the only citation source. The completed immutable Markdown note appears as a normal persisted assistant transcript entry with explicit source links. A later exact Markdown-export prompt opens the native save panel and appends one persisted file attachment without accepting or returning a path. Active research keeps one visible Stop action.",
    "missingBehavior": "Context-overflow repacking and stale-owner fault injection remain automated rather than packaged UI evidence. Stage A still requires exact attached Browser text and produces Markdown only; screenshots add visual evidence but not a citation identity. The fixed Qwen2-VL 2B model is not reliable enough for the strict research tool envelope on the packaged Wikipedia fixture, so the showcase uses Qwen2-VL for ordinary screenshot chat and Qwen Coder for research. It has no URL fetch, web search, Browser actions, file/memory/topic/link sources, arbitrary tools, shell, patches, DOCX, or slides. Stage B network access and Stage C search are candidate-only.",
    "frontendReachability": "Ordinary Chat prompts for Research and later Markdown export, inline progress/Stop, a normal assistant note, source links, and one exported Markdown attachment. No research selector, card, tabs, disclosure, or always-visible export control.",
    "backendReachability": "research.start, research.cancel, research.listArtifacts, research.loadArtifact, research.exportArtifact, and sequenced research/event frames.",
    "automatedEvidence": [
      "src-tauri/src/research/run_tests.rs",
      "src-tauri/src/research/model_tests.rs",
      "src-tauri/src/research/evidence_tests.rs",
      "src-tauri/src/research/context_tests.rs",
      "src-tauri/src/research/citations_tests.rs",
      "src-tauri/src/research/bundle_tests.rs",
      "src-tauri/src/research/export_tests.rs",
      "src-tauri/src/commands/research_tests.rs",
      "src/features/research/useResearchRun.test.tsx",
      "src/features/research/ResearchProgress.test.tsx",
      "src/features/chat/ChatEntryRow.test.tsx",
      "src/features/research/SafeMarkdownPreview.test.tsx",
      "src/features/chat/ChatPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "hardware: Apple Silicon macOS 27.0 beta packaged smoke recorded in docs/SMOKE_TESTING.md. Apple naturally exercised bounded malformed-framing recovery, produced an ordinary review-needed artifact in 4 turns / 8 calls, exported exact Markdown through NSSavePanel, and restored it after relaunch. Fixed Qwen Coder was explicitly downloaded, hash-verified, started through bundled MLX-LM, and produced a review-needed feathered-dinosaur note from the exact attached Simple English Wikipedia page; its source link rendered in chat and an explicit export prompt produced plume-feathered-dinosaurs.md through NSSavePanel. The packaged source candidate based on 813063300082acba919674bc638d5556d391a9bb let Qwen2-VL identify the exact 1064x1088 Browser screenshot in 7.4 seconds / 14 tokens, but two strict research attempts on that page failed closed after malformed-envelope recovery; this remains candidate evidence until repeated on an immutable implementation SHA. Stop reached its terminal; final head 5c88b2f fixes the packaged feedback defect that previously left the completed step text visible over the stopped status. Packaged transcript-native implementation head c4f1438d7efb2f3e6b44ebe0067504d8d3d6adc9 verified at 1152x768 that a restored completed note appears as a normal assistant reply with one source link and no research card, selectors, tabs, disclosure, or export controls; the source link opened example.com in the exact chat-owned Browser, an explicit export prompt opened the native Export research note panel with research-note.md, and cancellation returned quietly. Fault fixtures remain automated evidence.",
    "dependencies": ["persisted owning session", "1–10 exact owner-shelf Browser sources including at least one text capture", "available Apple system model, exact live fixed-Qwen Coder MLX handle, or exact live fixed-Qwen2-VL MLX-VLM handle"],
    "implementationPaths": [
      "src-tauri/src/research/",
      "src-tauri/src/commands/research.rs",
      "src/features/research/",
      "src/features/chat/ChatPanel.tsx",
      "src/lib/api/research.ts"
    ],
    "sourceDocuments": ["docs/AGENT_RUNTIME.md", "docs/IPC_CONTRACT.md", "docs/SAFETY.md", "docs/superpowers/specs/2026-07-19-provider-neutral-research-artifact-harness-design.md"],
    "nextCommissionedSlice": "Keep any Stage B network reader or Stage C search behind a separate reviewed design; do not broaden sources or tools implicitly",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "agent.single-step",
    "track": "agent-execution",
    "status": "partial",
    "currentBehavior": "One trusted Plume-managed MLX turn can fold an optional file, classify a diff, validate it, and hand it to explicit patch apply and revert with typed events. The separate bounded research-note workflow produces only inert Markdown artifacts and does not broaden coding-agent authority.",
    "missingBehavior": "The model cannot continue through a bounded read, edit, test, and fix loop or execute shell commands and arbitrary tools.",
    "frontendReachability": "Run one step panel, event log, proposed-change card, and explicit Apply/Revert controls.",
    "backendReachability": "agent.singleStep drives one MLX turn and patch.validate; user actions reuse patch.apply and patch.revert.",
    "automatedEvidence": [
      "src-tauri/src/commands/agent_command_tests.rs",
      "src/features/agent/AgentSingleStepPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "Apple Silicon in-app MLX single-step proof is documented; the deeper loop remains absent.",
    "dependencies": ["trusted project", "running Plume-managed MLX model", "propose-diff-or-higher agent mode"],
    "implementationPaths": [
      "src-tauri/src/commands/agent.rs",
      "src-tauri/src/agent/single_step.rs",
      "src/features/agent/AgentSingleStepPanel.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/SAFETY.md", "docs/IPC_ROADMAP.md"],
    "nextCommissionedSlice": "Connect the step to a bounded multi-iteration executor",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "agent.bounded-loop",
    "track": "agent-execution",
    "status": "scaffold",
    "currentBehavior": "A tested pure coding-agent controller models iteration budgets, pause, abort, failure, and completion outcomes. The production research-note controller is a separate narrow artifact workflow and is not this broad coding loop.",
    "missingBehavior": "No production IPC or UI connects the controller to a model, file tools, patch tools, verifier, or command executor.",
    "frontendReachability": "Agent settings expose mode, policy, allowlists, and iteration cap, but no loop run control exists.",
    "backendReachability": "Rust-only run_loop pure control flow; not called by production execution.",
    "automatedEvidence": ["src-tauri/src/agent/controller_tests.rs", "src/features/agent/AgentSettingsPanel.test.tsx"],
    "manualOrHardwareEvidence": "not applicable to scaffold",
    "dependencies": ["real step adapter", "approved command executor", "tool authorization gate"],
    "implementationPaths": [
      "src-tauri/src/agent/controller.rs",
      "src/features/agent/AgentSettingsPanel.tsx"
    ],
    "sourceDocuments": ["docs/SAFETY.md", "docs/IPC_ROADMAP.md"],
    "nextCommissionedSlice": "Wire real model, read, patch, and approved command steps",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "tools.catalog",
    "track": "skills-tools",
    "status": "scaffold",
    "currentBehavior": "Read-only list and search expose deterministic core and optional tool descriptions without granting authority.",
    "missingBehavior": "No arbitrary tools.invoke, MCP/plugin executor, or broad external tool authority exists.",
    "frontendReachability": "Typed API wrapper only; no general tool execution panel.",
    "backendReachability": "tools.list and tools.search return catalog metadata and execute nothing.",
    "automatedEvidence": [
      "src-tauri/src/agent/catalog_tests.rs",
      "src-tauri/src/commands/tools_tests.rs",
      "src/lib/api/tools.test.ts"
    ],
    "manualOrHardwareEvidence": "not applicable to scaffold",
    "dependencies": ["explicit approval gate", "allowlist", "bounded executor lifecycle"],
    "implementationPaths": [
      "src-tauri/src/agent/catalog.rs",
      "src-tauri/src/commands/tools.rs",
      "src/lib/api/tools.ts"
    ],
    "sourceDocuments": ["docs/TOOL_DISCLOSURE.md", "docs/IPC_CONTRACT.md"],
    "nextCommissionedSlice": "Put execution behind explicit approval and allowlist gates",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "providers.mlx-managed",
    "track": "local-models",
    "status": "shipped",
    "currentBehavior": "Packaged releases carry an identity-checked relocatable Python runtime with pinned MLX-LM and MLX-VLM and disable Python bytecode writes so runtime startup cannot mutate signed app resources. Fixed Qwen Coder 1.5B and Qwen2-VL 2B weights are separate explicit, pinned, resumable, hash-verified app-data downloads. The top-bar catalog can download, cancel, resume, verify, start, select, retry, and reuse either model without a project, Ollama, or user-managed Python. Only one fixed catalog model may start at once; switching stops the other running model first, and the backend rejects cross-window concurrent starts. Qwen2-VL accepts exact PNG screenshots in ordinary chat. Arbitrary compatible local folders retain the trusted-project MLX-LM path.",
    "missingBehavior": "No arbitrary catalog or silent model download is shipped; every upstream architecture is not guaranteed. Hard crashes, SIGKILL, and power loss run no child sweep, and persisted-PID adoption across Plume restarts remains unimplemented. Qwen Coder and Qwen2-VL chat do not supply the deeper read/edit/test agent loop or broad tools.",
    "frontendReachability": "One top-bar Model control opens three compact provider rows in an inline Models workspace: Apple On-Device, Qwen Coder, and Qwen2-VL. Advanced Local models inventory retains Start/Stop, running state, and diagnostics. Window-local selection and live handles survive local/project transitions, and running servers are re-adopted on webview reload.",
    "backendReachability": "providers.startServer, catalogStart, stopServer, serverDiagnostics, listServers, and MLX-routed chat.send; RunEvent::Exit sweep in lib.rs.",
    "automatedEvidence": [
      "src-tauri/src/providers/mlx_lm/process_tests.rs",
      "src-tauri/src/providers/mlx_runtime_tests.rs",
      "src-tauri/src/providers/catalog_tests.rs",
      "src-tauri/src/providers/catalog_download_tests.rs",
      "src-tauri/src/commands/providers_catalog_download.rs",
      "src-tauri/src/commands/providers_tests.rs",
      "src-tauri/src/chat/mlx_lm_tests.rs",
      "scripts/model-runtime-packaging.test.ts",
      "src/features/model-picker/ModelChooser.test.tsx",
      "src/features/model-picker/useModelCatalog.test.tsx",
      "src/features/providers/LocalModelsPanel.test.tsx",
      "src/features/providers/useMlxServers.test.tsx"
    ],
    "manualOrHardwareEvidence": "hardware: fixed Qwen Coder has packaged download, startup, generation, research, export, and normal-Quit evidence. In a packaged source candidate based on 813063300082acba919674bc638d5556d391a9bb, the fixed Qwen2-VL 2B exact 13-file `mlx-community/Qwen2-VL-2B-Instruct-4bit` revision `01af461cdb9574acc09084a0ef94e216e142b085` downloaded, hash-verified, started through the bundled MLX-VLM runtime, and answered the exact 1064x1088 Plume Browser PNG with `The screenshot shows Wikipedia's Simple English page about feathered dinosaurs.` in 7.4 seconds / 14 tokens. Direct-runtime peak memory was 2.205 GB. Packaged provider switching unloaded it before Qwen Coder started. Its strict research framing failed closed on the same Wikipedia fixture, so research/export evidence belongs to Qwen Coder rather than Qwen2-VL. Repeat the packaged candidate matrix after commit before calling it exact-head release evidence.",
    "dependencies": ["Apple Silicon for the happy path", "bundled release MLX runtime or debug interpreter", "compatible local model folder or receipt-backed Qwen Coder/Qwen2-VL", "trusted project for arbitrary local-model starts"],
    "implementationPaths": [
      "src-tauri/src/providers/mlx_lm/process.rs",
      "src-tauri/src/providers/mlx_runtime.rs",
      "src-tauri/src/providers/catalog.rs",
      "src-tauri/src/providers/catalog_download.rs",
      "src-tauri/src/commands/providers.rs",
      "src-tauri/src/chat/mlx_lm.rs",
      "src-tauri/build.rs",
      "src/features/providers/LocalModelsPanel.tsx",
      "src/features/model-picker/useModelCatalog.ts",
      "src/features/model-picker/ModelChooser.tsx",
      "scripts/build-mlx-runtime.sh"
    ],
    "sourceDocuments": ["docs/MODEL_PROVIDERS.md", "docs/MLX_RUNTIME.md", "docs/LOCAL_AGENT_NORTH_STAR.md"],
    "nextCommissionedSlice": "Keep MLX additions evidence-led; no broader agent loop is implied by vision model onboarding",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "benchmarks.evidence",
    "track": "benchmark-evidence",
    "status": "shipped",
    "currentBehavior": "Deterministic fixtures, strict records, verified MLX and Plume orchestration paths, resource probes, catalogs, presets, summaries, and a read-only in-app viewer are reachable.",
    "missingBehavior": "The full target-hardware matrix and evidence-backed launch claims have not been produced.",
    "frontendReachability": "Benchmarks workspace reads trusted project artifacts and catalogs without launching runs.",
    "backendReachability": "Terminal benchmark scripts run bounded harness paths; the viewer uses existing trusted display reads.",
    "automatedEvidence": [
      "scripts/benchmark/harness.test.ts",
      "scripts/benchmark/plume-orchestration.test.ts",
      "src/features/benchmarks/BenchmarksPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "hardware: pending for the full 128 GB M5 Max matrix; local smoke scripts prove mechanics only.",
    "dependencies": ["local model artifact with pinned identity", "target hardware for publishable matrix", "sanitized benchmark-artifacts records"],
    "implementationPaths": [
      "scripts/benchmark/run-model.ts",
      "scripts/benchmark/matrix.ts",
      "src/features/benchmarks/BenchmarksPanel.tsx"
    ],
    "sourceDocuments": ["docs/MODEL_BENCHMARKS.md", "docs/BENCHMARK_HARNESS.md"],
    "nextCommissionedSlice": "Run the full matrix on target hardware before D130 claims",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "library.workspace",
    "track": "library",
    "status": "shipped",
    "currentBehavior": "Library exposes two actionable overview summaries: About you opens app-private memory with or without a project, while This project opens trusted-project memory and Topics when available. It also provides scope-bounded lexical search, exact stored links/backlinks, independent retries, and click-or-drag placement of eligible opaque refs.",
    "missingBehavior": "Library has no graph, semantic retrieval, automatic prompt selection, automatic topic generation, cross-project aggregation, distillation, or background dreaming.",
    "frontendReachability": "Library in the unified sidebar starts with the actionable About you and This project scope summaries; browsing is read-only, Settings Library owns mutations, and Use in chat or typed drag adds only the selected opaque ref to an eligible owning chat.",
    "backendReachability": "Library reads independent app-private/project stores; chat resolves userMemoryEntry, memoryEntry, and topicFile refs only through their owning bounded resolver.",
    "automatedEvidence": [
      "src/features/library/projection.test.ts",
      "src/features/library/useLibraryData.test.tsx",
      "src/features/library/LibraryPanel.test.tsx",
      "src/features/library/LibrarySettingsPanel.test.tsx",
      "src/features/chat/ContextDropSurface.test.tsx",
      "src/App.test.tsx"
    ],
    "manualOrHardwareEvidence": "manual: packaged Calm UI implementation head 4a4e329a5e33bf2103b3f372b9d7a7a70aa8ecc0 at 1152x768 verified projectless Browse About you plus the existing Open project form, then a trusted disposable project opened This project and Topics as separate Library sources. Merely browsing either source added no chat shelf context. At that viewport, the Library overview exposed the two actions plainly with no cropped controls, bad wrapping, inconsistent spacing, or invisible keyboard focus. Packaged final-review implementation head 2b42926fbeb4cce0f7540fd0e1f8f50c6c2fc0a8 rechecked the projectless overview through Computer Use: About you showed its stored-item count and Browse action, while This project honestly remained unavailable and offered Open project.",
    "dependencies": ["app-data user-memory store", "trusted project for project memory/topics", "typed context shelf"],
    "implementationPaths": [
      "src/features/library/projection.ts",
      "src/features/library/useLibraryData.ts",
      "src/features/library/LibraryPanel.tsx",
      "src/features/library/LibraryWorkspace.tsx",
      "src/features/library/LibrarySettingsPanel.tsx",
      "src/features/chat/ContextDropSurface.tsx",
      "src/App.tsx"
    ],
    "sourceDocuments": [
      "docs/ROADMAP.md",
      "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"
    ],
    "nextCommissionedSlice": "No automatic retrieval slice commissioned",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "browser.session-foundation",
    "track": "browser-computer-use",
    "status": "shipped",
    "currentBehavior": "Schema v5 and main-webview-only browser.workspaceLoad/Save/Reset persist bounded per-chat layout, tabs, admitted top-level history, restoration status, and backend-sanitized URL descriptors in physically separate local/project session stores. Saves merge frontend-owned layout/tab shape with backend-owned native history atomically, so a stale layout request cannot erase a WebKit navigation that committed first. The integrated task Browser consumes that state. Secret-bearing URL tails remain marked for explicit manual reopen across later saves; ordinary navigation cannot clear the gate. Corrupt Browser rows reset without transcript loss and surface recovery actions; fork/rewind start empty. Local evidence is app-private and session-owned; tombstone reconciliation restores interrupted pre-commit deletes, purges committed orphans, and existence-only cleanup lets users delete chats with corrupt transcript children. An app-data advisory process lock covers reconciliation, evidence access, and composite deletion; unsupported platforms fail closed.",
    "missingBehavior": "No cross-chat Browser sharing, cookie/session export, or silent reopening of privacy-reduced URLs is shipped.",
    "frontendReachability": "The current local or project chat opens its own Browser workspace; its split/expanded layout, tabs, address draft, admitted history, restoration notice, and explicit reopen action restore only with that chat.",
    "backendReachability": "browser.workspaceLoad, browser.workspaceSave, and browser.workspaceReset are registered for webview main only and accept nested session identity rather than paths.",
    "automatedEvidence": [
      "src-tauri/src/sessions/browser_workspace_tests.rs",
      "src-tauri/src/browser/restoration_tests.rs",
      "src-tauri/src/browser/local_evidence_tests.rs",
      "src-tauri/src/commands/browser_workspace_tests.rs",
      "src/lib/api/browserWorkspace.test.ts",
      "src/features/browser/useTaskBrowser.test.tsx",
      "src/features/browser/useBrowserWorkspace.test.tsx",
      "src/features/browser/BrowserPanel.test.tsx",
      "src/App.test.tsx"
    ],
    "manualOrHardwareEvidence": "Packaged Plume Smoke.app on 2026-07-15 restored the same task's public page, tabs, address draft, and expanded layout across rebuild/relaunch; a different chat remained isolated. Split/expanded/return, native-child focus transfer, and accessibility-visible Browser controls were exercised physically.",
    "dependencies": ["persisted local/project chat session", "trusted open project for project scope"],
    "implementationPaths": [
      "src-tauri/src/sessions/schema.rs",
      "src-tauri/src/sessions/browser_workspace.rs",
      "src-tauri/src/sessions/browser_workspace_merge.rs",
      "src-tauri/src/browser/restoration.rs",
      "src-tauri/src/browser/local_evidence.rs",
      "src-tauri/src/commands/browser_workspace.rs",
      "src/lib/api/browserWorkspace.ts"
    ],
    "sourceDocuments": [
      "docs/IPC_CONTRACT.md",
      "docs/SAFETY.md",
      "docs/superpowers/specs/2026-07-14-consumer-workspace-design.md",
      "docs/superpowers/plans/2026-07-14-session-browser-foundation.md"
    ],
    "nextCommissionedSlice": "Preserve per-chat ownership and manual-reopen privacy gates as Browser evolves",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "browser.workspace",
    "track": "browser-computer-use",
    "status": "shipped",
    "currentBehavior": "Browser is a first-class workspace owned by the exact persisted local or project chat that opened it. Source-link navigation requests carry that exact chat identity, mismatched requests are ignored, and an ordinary Browser open clears any earlier source request. Split mode keeps task chat beside the native WebKit page; expanded mode gives the page the main canvas while retaining a compact task composer, and both layout and resizer width persist per chat. Sparse visible chrome provides tabs, address, Back, Forward, Reload, layout, and an Attach menu for selected text, readable page text, or the visible screenshot. HTML overlays wait for acknowledged native suspension; failed or hung suspension deactivates the native Browser before overlays are reported safe, and a visible retry restarts the same task-owned runtime without granting new authority. Captures bind to the current page generation and owning chat, persist immutable bounded records, and place only opaque ids onto that chat's shelf. Screenshot PNGs come from native WKWebView visible-viewport capture, are fully decoded and bounded, and reach only an exact Ollama model freshly reporting vision capability or the fixed Qwen2-VL MLX-VLM model. Screenshot chips enter the chat shelf with a short reduced-motion-safe handoff animation. The browser-sandbox webview has no Plume command capability. Top-level URLs are capped at 8 KiB, stale callbacks and captures are discarded, and privacy-reduced restored URLs require a separate explicit reopen action.",
    "missingBehavior": "No subresource host filter, full-page screenshot, browser executor, hidden navigation, or browser action dispatch exists. A Rust-owned activation epoch/token checked by deactivate and suspend is a nonblocking hardening candidate for theoretically late same-session native commands after frontend deadlines; no production failure is claimed or reproduced.",
    "frontendReachability": "Browser opens from the tools-only Workspace views drawer for the selected chat, with split/expanded task layouts, per-chat tabs and restoration, recovery/manual-reopen notices, and a trusted-project Attach menu. Projectless capture stays app-private; project capture stays under the trusted project.",
    "backendReachability": "browser.workspaceLoad/Save/Reset plus browser.taskActivate/Deactivate/OpenTab/CloseTab/SelectTab/Navigate/Back/Forward/Reload/SetGeometry/CaptureText/CaptureScreenshot are registered for webview main only. The older browser.sandbox lifecycle remains isolated, captured records resolve through chat.context/chat.send, and there is no executor.",
    "automatedEvidence": [
      "src/features/project-shell/ToolDrawer.test.tsx",
      "src/features/browser/BrowserPanel.test.tsx",
      "src/features/browser/useTaskBrowser.test.tsx",
      "src/features/project-shell/supportedMinimumLayout.test.ts",
      "src/App.test.tsx",
      "src/lib/api/browser.test.ts",
      "src-tauri/src/agent/catalog_tests.rs",
      "src-tauri/src/app_commands.rs",
      "src-tauri/src/browser/authority_tests.rs",
      "src-tauri/src/browser/evidence_tests.rs",
      "src-tauri/src/browser/screenshot_evidence_tests.rs",
      "src-tauri/src/browser/policy.rs",
      "src-tauri/src/browser/state.rs",
      "src-tauri/src/browser/runtime.rs",
      "src-tauri/src/commands/task_browser.rs",
      "src-tauri/src/commands/task_browser_activation.rs",
      "src-tauri/src/commands/task_browser_tests.rs",
      "src-tauri/src/commands/browser_workspace.rs",
      "src-tauri/src/commands/browser.rs",
      "src-tauri/src/prompts/explicit_context_tests.rs",
      "src-tauri/src/sessions/context_tests.rs"
    ],
    "manualOrHardwareEvidence": "Packaged Plume Smoke.app verified the original isolation/capture path on 2026-07-14 and the integrated task workspace on 2026-07-15. The latter physically proved same-chat page/tab/layout restoration across rebuild/relaunch, split to expanded to split, address-draft persistence, native-child focus closing the Attach menu, accessibility-visible controls, and final side-by-side visual comparison against the approved Codex references. PR #152 recovery smoke on its final implementation tree, squash-merged as e57439257aafd7ca28c2d62f604b085ade540a22, additionally proved Settings, Help, Workspace views, and Rename render above an active task Browser, then a quit/relaunch restores a valid 532 px split descriptor. Packaged final-review implementation head 2b42926fbeb4cce0f7540fd0e1f8f50c6c2fc0a8 loaded example.com in a local chat Browser, suspended the native child under Settings, restored that visible page on close, then after normal Quit/relaunch restored the same chat's example.com tab and address descriptor when Browser was reopened. Packaged shell-cleanup implementation head 9243b504640087f308112a5b7ed0c9045ef97dbe verified the Workspace views drawer now exposes only Files, Browser, and Benchmarks with stable accessibility names and focus entry; it did not repeat native page restoration. Packaged transcript-native implementation head c4f1438d7efb2f3e6b44ebe0067504d8d3d6adc9 verified that the visible Example Domain source link opened example.com inside the exact restored chat's split Browser. Fractional and below-minimum measurements remain deterministic component-test evidence rather than a claimed packaged interaction. The native child WebView uses a reserved compact composer row in expanded mode because it cannot safely share HTML z-order.",
    "dependencies": ["bounded evidence resolver before any prompt attachment", "guarded execution before agent actions"],
    "implementationPaths": [
      "src/features/project-shell/ToolDrawer.tsx",
      "src/features/browser/BrowserPanel.tsx",
      "src/features/browser/useTaskBrowser.ts",
      "src/lib/api/browser.ts",
      "src-tauri/src/agent/catalog.rs",
      "src-tauri/build.rs",
      "src-tauri/capabilities/default.json",
      "src-tauri/src/app_commands.rs",
      "src-tauri/src/browser/policy.rs",
      "src-tauri/src/browser/state.rs",
      "src-tauri/src/browser/runtime.rs",
      "src-tauri/src/browser/evidence.rs",
      "src-tauri/src/browser/native_snapshot.rs",
      "src-tauri/src/browser/screenshot_evidence.rs",
      "src-tauri/src/prompts/explicit_context.rs",
      "src-tauri/src/commands/browser.rs",
      "src-tauri/src/commands/browser_workspace.rs",
      "src-tauri/src/commands/task_browser.rs",
      "src-tauri/src/commands/task_browser_activation.rs"
    ],
    "sourceDocuments": [
      "docs/IPC_CONTRACT.md",
      "docs/ROADMAP.md",
      "docs/IPC_ROADMAP.md",
      "docs/superpowers/specs/2026-07-14-browser-isolation-proof-design.md",
      "docs/superpowers/specs/2026-07-14-human-browser-workspace-design.md",
      "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"
    ],
    "nextCommissionedSlice": "No agent-driven Browser action or Rust activation-epoch hardening slice commissioned",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "computer.external-operability",
    "track": "operability",
    "status": "shipped",
    "currentBehavior": "Plume exposes labeled visible controls, status, keyboard paths, modal focus handling, appearance-safe overlays, and recoverable workspace navigation that external computer-use agents can drive through ordinary OS accessibility. Workspace views contains tools only, archived chats have one Settings home, Help remains icon-labelled, project close sits in the project-row menu, compact Continue/Rewind actions retain a disclosed explanation, and research answers and exports use ordinary transcript entries instead of a separate control card. The Model control keeps a stable accessible name, returns focus, supports keyboard dismissal, exposes host/download/start failures visibly, and waits for native Browser suspension before its inline workspace appears.",
    "missingBehavior": "There is no private external automation API or promise that every future UI state is operable without continued accessibility testing.",
    "frontendReachability": "Unified top bar, tools-only Workspace views drawer, scoped sidebar and project menu, Settings Archived sections, chat-native research answers and attachments, dialogs, and visible status/error surfaces.",
    "backendReachability": "Not applicable; the receiving role uses the rendered Tauri UI and platform accessibility rather than computer-use IPC.",
    "automatedEvidence": [
      "src/features/project-shell/UnifiedChrome.test.tsx",
      "src/features/model-picker/ModelChooser.test.tsx",
      "src/features/project-shell/ToolDrawer.test.tsx",
      "src/features/chat/ChatPanel.test.tsx",
      "src/features/chat/ContextShelf.test.tsx",
      "src/features/chat/ChatEntryRow.test.tsx",
      "src/features/appearance/AppearancePanel.test.tsx",
      "src/features/help/HelpPanel.test.tsx",
      "src/features/sessions/SessionDialogs.test.tsx",
      "src/App.test.tsx"
    ],
    "manualOrHardwareEvidence": "Packaged Build Week candidate smoke at 2a3520e verified the earlier context and workspace surfaces. Packaged Calm UI implementation head 4a4e329a5e33bf2103b3f372b9d7a7a70aa8ecc0 at 1152x768 verified the two-row Model chooser, forward/backward Tab containment, Escape focus restoration, outside-click dismissal, Apple and Qwen selection, and Settings, Help, and Workspace views above an active Browser through ordinary OS accessibility. The matched exact-viewport visual review found no cropped control, bad wrapping, inconsistent spacing, unnecessary nested border, invisible keyboard focus, or harder-to-understand smaller state. Packaged final-review implementation head 2b42926fbeb4cce0f7540fd0e1f8f50c6c2fc0a8 rechecked the compact Apple/Qwen rows through Computer Use: forward and reverse Tab wrapped inside the chooser, Escape restored Model focus, and a chat-canvas click dismissed the chooser. Apple selection and Qwen Starting-to-selected transition both completed. Packaged shell-cleanup implementation head 9243b504640087f308112a5b7ed0c9045ef97dbe verified the tools-only Workspace views drawer and Settings Archived category at 1152x768 through ordinary accessibility, including readable archive rows and action labels; the project-row overflow remained component-test evidence because the packaged trust confirmation did not activate through the external driver. Packaged transcript-native implementation head c4f1438d7efb2f3e6b44ebe0067504d8d3d6adc9 verified at 1152x768 through ordinary accessibility that the restored research note is a normal reply with one source link and no research controls, the link opens the exact chat-owned Browser, and export appears only after an explicit prompt through the native save panel. The unavailable-focused-action transition is covered by the controlled component regression, not claimed as a native packaged interaction.",
    "dependencies": ["rendered Tauri window", "OS accessibility, keyboard, or mouse input"],
    "implementationPaths": [
      "src/features/project-shell/UnifiedChrome.tsx",
      "src/features/model-picker/ModelChooser.tsx",
      "src/features/project-shell/ToolDrawer.tsx",
      "src/features/chat/ChatPanel.tsx",
      "src/features/chat/ContextShelf.tsx",
      "src/features/research/ResearchTranscriptEntry.tsx",
      "src/features/appearance/AppearancePanel.tsx",
      "src/features/help/HelpPanel.tsx",
      "src/features/sessions/SessionDialogs.tsx",
      "src/App.tsx"
    ],
    "sourceDocuments": ["docs/AGENT_OPERABILITY.md", "docs/PLUME_PROJECT_SPEC.md"],
    "nextCommissionedSlice": "Keep new UI states accessible and recoverable",
    "lastVerifiedCommit": "a324b37684e94297c110d7ef3bb617233fded558",
    "lastVerifiedDate": "2026-07-21"
  },
  {
    "id": "computer.emitting-sandbox",
    "track": "browser-computer-use",
    "status": "researched",
    "currentBehavior": "The Browser capability-isolation floor now provides a browser-sandbox webview with no Plume authority. A separate Phase A contract defines target allowlists, foreground approval, visible trace, Pause and Stop, capture, observation, and no host access.",
    "missingBehavior": "No computer session, computer.* IPC, action executor, trace UI, approval or target allowlist, agent capture/observation, or input synthesis exists.",
    "frontendReachability": "None.",
    "backendReachability": "Browser sandbox lifecycle only; no computer-use session or action command is registered.",
    "automatedEvidence": ["src-tauri/src/browser/authority_tests.rs"],
    "manualOrHardwareEvidence": "Computer-use emission remains research; human Browser navigation smoke does not prove agent execution.",
    "dependencies": ["human Browser workspace", "guarded execution", "per-session approval and exact target allowlist", "visible trace and Pause/Stop"],
    "implementationPaths": ["src-tauri/src/browser/authority_tests.rs", "src-tauri/src/commands/browser.rs"],
    "sourceDocuments": ["docs/IPC_ROADMAP.md", "docs/SAFETY.md", "docs/AGENT_OPERABILITY.md"],
    "nextCommissionedSlice": "Finish guarded execution, per-session approval, target allowlist, and visible trace before a bounded action",
    "lastVerifiedCommit": "56e53e00dca140f9b13eb42ea8fbde7f3920f6fe",
    "lastVerifiedDate": "2026-07-14"
  },
  {
    "id": "computer.host-control",
    "track": "browser-computer-use",
    "status": "researched",
    "currentBehavior": "A separate Phase B macOS host-control contract documents app-level OS permissions, project trust, per-session approval, exact target allowlists, and visible trace requirements.",
    "missingBehavior": "No Accessibility or Screen Recording integration, CGEvent input, window capture, host executor, or host-control UI exists.",
    "frontendReachability": "None.",
    "backendReachability": "None.",
    "automatedEvidence": [],
    "manualOrHardwareEvidence": "research and safety design only",
    "dependencies": ["proven Phase A sandbox", "macOS accessibility permission", "macOS screen-recording permission", "per-session host approval"],
    "implementationPaths": [],
    "sourceDocuments": ["docs/SAFETY.md", "docs/AGENT_OPERABILITY.md", "docs/IPC_ROADMAP.md"],
    "nextCommissionedSlice": "Revisit only after sandbox execution and safety evidence",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  }
]
```
