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
| Streaming chat | shipped | Ollama and Plume-managed MLX stream cancellable token events into the chat UI. | Keep new provider adapters on the same event contract. |
| Session persistence | shipped | Local and trusted-project chats persist bounded transcripts and FTS search in separate SQLite stores. | Add migration/export tooling only when commissioned. |
| Session branching | shipped | Users can continue or rewind into a new persisted chat with provenance. | Add branch comparison or merge only when commissioned. |
| Project trust and context | shipped | Persisted trust gates project instructions plus a sticky typed shelf of exact file/selection, memory-entry, topic-file, and immutable Browser-text refs; visible drag/drop reuses that contract. | Add screenshot evidence only with a bounded image resolver and honest model path. |
| Exact context manifest | shipped | Sends, previews, and persisted user turns report the exact ordered explicit sources accepted by prompt assembly, including captured Browser text provenance. | Extend the contract to screenshots only when image prompt support exists. |
| Safe patch lifecycle | shipped | Validated diffs apply atomically through checkpoints and can be drift-checked and reverted. | Keep broader writes behind separate approval and allowlist gates. |
| Memory entries | shipped | Users can create, read, update, forget, search, and inject bounded redacted notes. | Expose entries in the Knowledge workspace. |
| Memory topics | shipped | Validated curated Markdown topics are browsable and the core trio feeds bounded prompt context. | Add read-only topic navigation and backlinks. |
| Memory links | shipped | Users link remembered entries to validated curated topic files as organization metadata. | Add Knowledge workspace backlinks. |
| Memory distillation | shipped | Users preview and apply exact-duplicate compaction with stale protection, link inheritance, and audit history. | Keep LLM-assisted summaries separate and opt-in. |
| Semantic memory retrieval | researched | Staged local semantic retrieval is documented. | Build an evaluation set after lexical preview and explicit insertion exist. |
| Project skill library | shipped | Trusted projects can list, inspect, preview, and explicitly write bounded skill files. | Add automatic improvement only behind reviewable drafts. |
| Session skill promotion | shipped | Selected project-chat messages become a redacted, snapshot-checked editable skill draft. | Preserve source provenance in later skill-improvement flows. |
| Agent single step | partial | A trusted MLX turn can validate a proposed diff and hand it to explicit patch apply/revert. | Connect the step to a bounded multi-iteration executor. |
| Bounded agent loop | scaffold | A tested pure controller models budget, pause, abort, failure, and completion. | Wire real model, read, patch, and approved command steps. |
| Tool catalog | scaffold | Read-only list/search exposes core and optional tool descriptions. | Put execution behind explicit approval and allowlist gates. |
| Plume-managed MLX | shipped | Trusted projects can discover, start, select, stream from, inspect, and stop MLX-LM servers. | Keep MLX-LM the happy path and add models only with evidence. |
| Benchmark evidence | shipped | Deterministic harnesses, verified MLX/Plume paths, catalogs, presets, and a read-only viewer are reachable. | Run the full matrix on target hardware before D130 claims. |
| Knowledge workspace | shipped | Trusted projects expose capped topics, exact-ref backlinks, lexical search, and click-or-drag placement of opaque memory/topic refs. | Keep retrieval automatic only after an evaluated preview milestone. |
| Browser workspace | shipped | A global human workspace controls one separately labelled incognito HTTP(S) window with visible navigation state, fixed controls, exact-origin localhost confirmation, and explicit trusted-project text capture. | Add screenshots separately without granting agent navigation authority. |
| External computer operability | shipped | Labeled visible controls and status let external agents drive Plume through ordinary OS accessibility paths. | Keep new UI states accessible and recoverable. |
| Computer-use sandbox emission | researched | The capability-isolated human Browser exists, while agent approvals, target allowlist, trace, Pause/Stop, capture, and action execution remain research. | Add evidence first; require guarded execution gates before a bounded action. |
| Computer host control | researched | Separate opt-in macOS host-control gates are documented. | Revisit only after sandbox execution and safety evidence. |

```inventory-json
[
  {
    "id": "chat.streaming",
    "track": "local-chat",
    "status": "shipped",
    "currentBehavior": "Ollama and Plume-managed MLX stream cancellable token events into the chat UI.",
    "missingBehavior": "Additional provider adapters must still adopt the same streaming event contract.",
    "frontendReachability": "Chat workspace composer, transcript, streaming cursor, and Stop control.",
    "backendReachability": "chat.send and chat.cancel dispatch through the Ollama or MLX-LM streaming adapter.",
    "automatedEvidence": [
      "src-tauri/src/commands/chat/send_tests.rs",
      "src-tauri/src/chat/mlx_lm_tests.rs",
      "src/features/chat/ChatPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "Apple Silicon MLX chat smoke is documented; not required for implementation status.",
    "dependencies": ["selected reachable local model", "Ollama compatibility runtime or Plume-managed MLX handle"],
    "implementationPaths": [
      "src-tauri/src/commands/chat/send.rs",
      "src-tauri/src/chat/mlx_lm.rs",
      "src/features/chat/useChat.ts"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/MODEL_PROVIDERS.md"],
    "nextCommissionedSlice": "Keep new provider adapters on the same event contract",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "sessions.persistence",
    "track": "sessions",
    "status": "shipped",
    "currentBehavior": "Local and trusted-project chats persist bounded transcripts and FTS search in separate SQLite stores.",
    "missingBehavior": "No cross-device sync or export workflow is shipped.",
    "frontendReachability": "Session sidebar, archived chats, search overlay, and stable-boundary transcript saves.",
    "backendReachability": "sessions.list, create, load, rename, archive, delete, saveTranscript, and search.",
    "automatedEvidence": [
      "src-tauri/src/sessions/tests.rs",
      "src/features/sessions/usePersistedChat.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["app-data directory for local chats", "trusted project for project chats"],
    "implementationPaths": [
      "src-tauri/src/sessions/mod.rs",
      "src-tauri/src/commands/sessions.rs",
      "src/features/sessions/usePersistedChat.ts"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/AGENT_OPERABILITY.md"],
    "nextCommissionedSlice": "No sync or export slice commissioned",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "sessions.branching",
    "track": "sessions",
    "status": "shipped",
    "currentBehavior": "Users can continue a full chat or rewind selected turns into a new persisted chat with parent and boundary provenance.",
    "missingBehavior": "Branches cannot be compared or merged back together.",
    "frontendReachability": "Continue in new chat and rewind-to-new-chat actions on persisted project sessions.",
    "backendReachability": "sessions.fork and sessions.rollback perform atomic transcript branches.",
    "automatedEvidence": [
      "src-tauri/src/sessions/fork_tests.rs",
      "src-tauri/src/sessions/rollback_tests.rs",
      "src/features/sessions/usePersistedChat.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["persisted project session", "trusted project"],
    "implementationPaths": [
      "src-tauri/src/sessions/branch.rs",
      "src-tauri/src/sessions/mod.rs",
      "src/features/sessions/usePersistedChat.ts"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/AGENT_OPERABILITY.md"],
    "nextCommissionedSlice": "No branch comparison or merge slice commissioned",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "project.trust-and-context",
    "track": "project-context",
    "status": "shipped",
    "currentBehavior": "Persisted project trust gates project instructions plus a sticky ordered shelf of exact project-file or selection, memory-entry, curated-topic, and immutable Browser-text refs; Files and Knowledge expose a temporary drag/drop target, while Browser exposes explicit text-capture buttons over the same shelf.",
    "missingBehavior": "Browser screenshots and automatic retrieval authority are not shipped.",
    "frontendReachability": "Project chat context shelf plus click-or-drag Use in chat controls in the inspector and Knowledge workspace, and Use selection/page text in the trusted-project Browser.",
    "backendReachability": "chat.context and chat.send resolve typed refs through their owning trusted bounded readers before any stream registration.",
    "automatedEvidence": [
      "src-tauri/src/project/trust.rs",
      "src-tauri/src/prompts/assemble_tests.rs",
      "src-tauri/src/prompts/explicit_context_tests.rs",
      "src-tauri/src/browser/evidence_tests.rs",
      "src/features/chat/ChatPanel.test.tsx",
      "src/features/chat/ContextDropSurface.test.tsx",
      "src/features/chat/contextDragPayload.test.ts",
      "src/features/chat/useChat.test.tsx",
      "src/features/browser/BrowserPanel.test.tsx",
      "src/features/sessions/usePersistedChat.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["open project", "persisted trust decision"],
    "implementationPaths": [
      "src-tauri/src/commands/project.rs",
      "src-tauri/src/project/trust.rs",
      "src-tauri/src/prompts/assemble.rs",
      "src-tauri/src/prompts/explicit_context.rs",
      "src-tauri/src/browser/evidence.rs",
      "src/features/chat/ContextShelf.tsx",
      "src/features/chat/ContextDropSurface.tsx"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/SAFETY.md"],
    "nextCommissionedSlice": "Screenshot evidence only after bounded snapshot storage and an honest image prompt path",
    "lastVerifiedCommit": "0d69471ceaf399198b06c60775c50d8f8be07ce4",
    "lastVerifiedDate": "2026-07-14"
  },
  {
    "id": "context.exact-manifest",
    "track": "project-context",
    "status": "shipped",
    "currentBehavior": "Chat preview, send acceptance, and persisted user turns carry the exact ordered file, memory-entry, topic-file, and immutable Browser-text sources accepted by bounded prompt assembly.",
    "missingBehavior": "Browser screenshots and other future source kinds are not yet part of the manifest.",
    "frontendReachability": "Per-source shelf readiness plus immutable accepted-context chips on user turns.",
    "backendReachability": "chat.context resolves per-source outcomes and chat.send returns the accepted explicit manifest before the user turn becomes persistable.",
    "automatedEvidence": [
      "src-tauri/src/prompts/assemble_tests.rs",
      "src-tauri/src/commands/chat/send_tests.rs",
      "src-tauri/src/prompts/explicit_context_tests.rs",
      "src/features/chat/useChat.test.tsx",
      "src/features/sessions/usePersistedChat.test.tsx"
    ],
    "manualOrHardwareEvidence": "not required",
    "dependencies": ["trusted project", "bounded prompt assembly"],
    "implementationPaths": [
      "src-tauri/src/prompts/explicit_context.rs",
      "src-tauri/src/commands/chat/context.rs",
      "src-tauri/src/commands/chat/send.rs"
    ],
    "sourceDocuments": ["docs/IPC_CONTRACT.md", "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"],
    "nextCommissionedSlice": "Carry screenshots only after their owning bounded resolver and model path ship",
    "lastVerifiedCommit": "0d69471ceaf399198b06c60775c50d8f8be07ce4",
    "lastVerifiedDate": "2026-07-14"
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
    "id": "memory.entries",
    "track": "project-knowledge",
    "status": "shipped",
    "currentBehavior": "Users can create, read, update, forget, text-search, and prompt-inject bounded redacted project memory entries.",
    "missingBehavior": "Entries have no semantic retrieval, automatic contradiction handling, or background dreaming.",
    "frontendReachability": "Memory settings entry list, editor, search, and Forget actions.",
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
    "nextCommissionedSlice": "Expose entries in the Knowledge workspace",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "memory.topics",
    "track": "project-knowledge",
    "status": "shipped",
    "currentBehavior": "Users can browse validated curated Markdown topics, while non-empty INDEX.md, USER.md, and SOUL.md feed bounded prompt context.",
    "missingBehavior": "There is no dedicated Knowledge navigation or backlink projection.",
    "frontendReachability": "Memory settings Topic files disclosure and chat Topics badge.",
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
    "nextCommissionedSlice": "Read-only Knowledge topic navigation and backlinks",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "memory.links",
    "track": "project-knowledge",
    "status": "shipped",
    "currentBehavior": "Users link remembered entries to validated curated topic files.",
    "missingBehavior": "Links do not select prompt context or semantic retrieval.",
    "frontendReachability": "Memory settings link editor.",
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
    "nextCommissionedSlice": "Knowledge workspace backlinks",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
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
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
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
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "agent.single-step",
    "track": "agent-execution",
    "status": "partial",
    "currentBehavior": "One trusted Plume-managed MLX turn can fold an optional file, classify a diff, validate it, and hand it to explicit patch apply and revert with typed events.",
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
    "currentBehavior": "A tested pure controller models iteration budgets, pause, abort, failure, and completion outcomes.",
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
    "currentBehavior": "Trusted projects can discover compatible local folders, start and stop supervised MLX-LM servers, select them, stream chat, and inspect diagnostics.",
    "missingBehavior": "Plume does not install mlx-lm, download models, or guarantee every transformer architecture is supported upstream.",
    "frontendReachability": "Local models inventory and selected-model Start/Stop, running-state, and diagnostics controls.",
    "backendReachability": "providers.startServer, stopServer, serverDiagnostics, and MLX-routed chat.send.",
    "automatedEvidence": [
      "src-tauri/src/providers/mlx_lm/process_tests.rs",
      "src-tauri/src/chat/mlx_lm_tests.rs",
      "src/features/providers/LocalModelsPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "Apple Silicon Qwen MLX chat and propose-diff smokes are documented; hardware proof is independent from shipped status.",
    "dependencies": ["Apple Silicon for the happy path", "user-installed mlx-lm interpreter", "compatible local model folder", "trusted project to spawn"],
    "implementationPaths": [
      "src-tauri/src/providers/mlx_lm/process.rs",
      "src-tauri/src/chat/mlx_lm.rs",
      "src/features/providers/LocalModelsPanel.tsx"
    ],
    "sourceDocuments": ["docs/MODEL_PROVIDERS.md", "docs/MLX_RUNTIME.md", "docs/LOCAL_AGENT_NORTH_STAR.md"],
    "nextCommissionedSlice": "Keep MLX-LM the happy path and add models only with evidence",
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
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
    "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "knowledge.workspace",
    "track": "project-knowledge",
    "status": "shipped",
    "currentBehavior": "Trusted projects expose capped topic navigation, exact-ref memory backlinks, unlinked and stale-linked views, lexical search, and click-or-drag placement of memory entries and curated topic files into project chat.",
    "missingBehavior": "The workspace cannot perform semantic retrieval, generate topics, or mutate memory.",
    "frontendReachability": "Knowledge in the trusted Workspace views drawer; Use in chat or its temporary drag target switches to project chat and adds only an opaque typed ref.",
    "backendReachability": "Knowledge remains read-only; chat resolves selected memory/topic refs through the existing owning stores.",
    "automatedEvidence": [
      "src/features/knowledge/projection.test.ts",
      "src/features/knowledge/useKnowledgeData.test.tsx",
      "src/features/knowledge/KnowledgePanel.test.tsx",
      "src/features/chat/ContextDropSurface.test.tsx",
      "src/App.test.tsx"
    ],
    "manualOrHardwareEvidence": "Packaged-app Knowledge smoke is required for the UI slice; no model or special hardware is required.",
    "dependencies": ["trusted project", "bounded memory.index and memory.topics reads"],
    "implementationPaths": [
      "src/features/knowledge/projection.ts",
      "src/features/knowledge/useKnowledgeData.ts",
      "src/features/knowledge/KnowledgePanel.tsx",
      "src/features/chat/ContextDropSurface.tsx",
      "src/App.tsx"
    ],
    "sourceDocuments": [
      "docs/ROADMAP.md",
      "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"
    ],
    "nextCommissionedSlice": "No automatic retrieval slice commissioned",
    "lastVerifiedCommit": "761b9770a91ed4e7c9007328535d8ae454357264",
    "lastVerifiedDate": "2026-07-13"
  },
  {
    "id": "browser.workspace",
    "track": "browser-computer-use",
    "status": "shipped",
    "currentBehavior": "Browser is a global Workspace view in project and no-project shells. Its sparse human controls own one incognito HTTP(S) browser-sandbox window with visible URL/loading/failure state, fixed Back/Forward/Reload/Show/Close actions, and exact-origin localhost approval once per window session. In a trusted project, fixed Use selection/page text actions bind capture to the current page generation and project, redact and persist an immutable bounded record, and place only its opaque id onto project chat. The main webview has an explicit application-command allowlist; browser-sandbox matches no capability, and runtime tests prove it cannot invoke app or core-event commands. Top-level URLs are capped at 8 KiB; stale callbacks and captures are discarded; title stays null in navigation state. macOS clipboard access remains enabled by the embedded-browser default; general-autofill and extension toggles are unsupported no-ops there.",
    "missingBehavior": "No screenshot capture, subresource host filter, browser executor, hidden navigation, or browser action dispatch exists.",
    "frontendReachability": "Browser in the Workspace views drawer from simple-chat, untrusted-project, and trusted-project shells; text capture enables only for a trusted project.",
    "backendReachability": "browser.sandboxOpen/Close/State/Focus/Back/Forward/Reload/CaptureText are registered for webview main only; captured records resolve through chat.context/chat.send, and there is no executor.",
    "automatedEvidence": [
      "src/features/project-shell/ToolDrawer.test.tsx",
      "src/features/browser/BrowserPanel.test.tsx",
      "src/features/browser/useBrowserWorkspace.test.tsx",
      "src/lib/api/browser.test.ts",
      "src-tauri/src/agent/catalog_tests.rs",
      "src-tauri/src/app_commands.rs",
      "src-tauri/src/browser/authority_tests.rs",
      "src-tauri/src/browser/evidence_tests.rs",
      "src-tauri/src/browser/policy.rs",
      "src-tauri/src/browser/state.rs",
      "src-tauri/src/commands/browser.rs",
      "src-tauri/src/prompts/explicit_context_tests.rs",
      "src-tauri/src/sessions/context_tests.rs"
    ],
    "manualOrHardwareEvidence": "Packaged Plume Smoke.app verified 2026-07-14 against disposable /private/tmp localhost fixtures: projectless and trusted-project reachability, exact-origin prompt/cancel/confirm, separate system webview, live URL state, Back/Forward/Reload/Show/Close, close-then-reapproval, public HTTPS without approval, physical page selection, and end-to-end page-text capture into one persisted opaque Browser evidence ref. The smoke controller clears DOM selection while switching windows through the macOS menu, so selection capture remains covered by the shared callback and command tests rather than claimed as a combined physical click.",
    "dependencies": ["bounded evidence resolver before any prompt attachment", "guarded execution before agent actions"],
    "implementationPaths": [
      "src/features/project-shell/ToolDrawer.tsx",
      "src/features/browser/BrowserPanel.tsx",
      "src/features/browser/useBrowserWorkspace.ts",
      "src/lib/api/browser.ts",
      "src-tauri/src/agent/catalog.rs",
      "src-tauri/build.rs",
      "src-tauri/capabilities/default.json",
      "src-tauri/src/app_commands.rs",
      "src-tauri/src/browser/policy.rs",
      "src-tauri/src/browser/state.rs",
      "src-tauri/src/browser/evidence.rs",
      "src-tauri/src/prompts/explicit_context.rs",
      "src-tauri/src/commands/browser.rs"
    ],
    "sourceDocuments": [
      "docs/IPC_CONTRACT.md",
      "docs/ROADMAP.md",
      "docs/IPC_ROADMAP.md",
      "docs/superpowers/specs/2026-07-14-browser-isolation-proof-design.md",
      "docs/superpowers/specs/2026-07-14-human-browser-workspace-design.md",
      "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"
    ],
    "nextCommissionedSlice": "Add screenshot evidence only with bounded image storage and honest model support",
    "lastVerifiedCommit": "0d69471ceaf399198b06c60775c50d8f8be07ce4",
    "lastVerifiedDate": "2026-07-14"
  },
  {
    "id": "computer.external-operability",
    "track": "operability",
    "status": "shipped",
    "currentBehavior": "Plume exposes labeled visible controls, status, keyboard paths, and recoverable workspace navigation that external computer-use agents can drive through ordinary OS accessibility.",
    "missingBehavior": "There is no private external automation API or promise that every future UI state is operable without continued accessibility testing.",
    "frontendReachability": "Unified top bar, Workspace views drawer, chat controls, dialogs, and visible status/error surfaces.",
    "backendReachability": "Not applicable; the receiving role uses the rendered Tauri UI and platform accessibility rather than computer-use IPC.",
    "automatedEvidence": [
      "src/features/project-shell/UnifiedChrome.test.tsx",
      "src/features/project-shell/ToolDrawer.test.tsx",
      "src/features/chat/ChatPanel.test.tsx"
    ],
    "manualOrHardwareEvidence": "Packaged-app smoke remains the final external-operability check for UI slices.",
    "dependencies": ["rendered Tauri window", "OS accessibility, keyboard, or mouse input"],
    "implementationPaths": [
      "src/features/project-shell/UnifiedChrome.tsx",
      "src/features/project-shell/ToolDrawer.tsx",
      "src/features/chat/ChatPanel.tsx"
    ],
    "sourceDocuments": ["docs/AGENT_OPERABILITY.md", "docs/PLUME_PROJECT_SPEC.md"],
    "nextCommissionedSlice": "Keep new UI states accessible and recoverable",
    "lastVerifiedCommit": "4cd5a07223d3555d107bfaf786d6712f0cd4251b",
    "lastVerifiedDate": "2026-07-13"
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
    "lastVerifiedCommit": "1cc8e4dc7d107c1b65a659595ae06039b81779f0",
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
