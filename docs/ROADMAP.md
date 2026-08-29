# Plume Roadmap

Status vocabulary comes from [FEATURE_INVENTORY.md](FEATURE_INVENTORY.md).
Research is not implementation. Slice numbers are assigned only when a slice is
commissioned.

## Commissioned Sequence

1. Documentation and agent navigation — shipped; compact-entrypoint
   maintenance is active when ownership maps or status pointers drift.
2. Typed explicit context shelf with manual Use in chat — shipped.
3. Drag/drop convenience for typed context sources — shipped.
4. Browser remote-content capability isolation — shipped.
5. Human-controlled Browser workspace — shipped.
6. Bounded Browser text evidence placement — shipped.
7. Bounded provider-neutral research notes — Stage A implementation and the
   normal packaged Apple/Qwen Coder path are complete; fixed Qwen2-VL can
   additionally inspect explicit screenshots. Its direct-runtime smoke is
   recorded, but its packaged-app research/export matrix remains to be recorded,
   so the feature stays partial.
8. Durable Home conversation — commissioned programme Phase 1.
9. Transparent provider-neutral compaction — commissioned programme Phase 2.
10. Reviewable learning — commissioned programme Phase 3.
11. Read-only multi-folder grants and removal of Projects from the normal
    consumer model — commissioned programme Phases 4–5.
12. One-writable-folder guarded task execution and the bounded
    read/edit/test/fix loop — commissioned programme Phases 6–7.
13. Qwen3.8-27B task-pipeline validation, followed by measured challenger
    comparisons — commissioned programme Phase 8, after the execution loop.
14. Computer-use emission inside the sandbox — later, after the guarded loop
    and one allowlisted tool path.

Items 8–13 form the approved
[Continuous Chat and Folder Grants programme](superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md).
They are ordered product commitments, not permission to ship one monster diff.
Each phase receives its own failing tests, implementation plan, focused branch,
verification, exact-head review, and merge gate.

The 128 GB M5 Max benchmark matrix runs when the hardware exists. The D130
launch rewrite follows measured evidence and does not block unrelated product
work.

## Track: Continuous Chat, Compaction, And Folder Grants

**Outcome:** Plume feels like one persistent local teammate. The user returns
to one durable Home conversation, old context compacts without deleting
history, useful learning is explicitly reviewed, and folders can be attached
when needed without creating or switching Projects.

**Approved design:**
[`docs/superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md`](superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md).

**Current floor:** Plume ships bounded persisted local and trusted-project
chats in physically separate SQLite stores. It ships explicit app-private user
memory and trusted-project memory, exact context manifests, fork/rewind, and
folder trust through the currently open project. It does not ship one Home
conversation, context compaction, learning proposals, multiple simultaneous
folder grants, or a chat-first replacement for Projects.

**Product decisions:**

- Remove Projects from the normal consumer vocabulary and navigation.
- Store new consumer conversation history in backend-owned app data rather
  than making a folder own the conversation.
- Represent folders as opaque Rust-owned grants. Attaching a folder permits
  bounded reads only and never implies write, command, Browser, or tool
  authority.
- Allow one writable folder per coding run. Additional folders are read-only;
  changing the writable folder requires a new visible approval and run lease.
- Preserve the full transcript. Compaction adds an inspectable derived
  checkpoint plus complete recent turns; it never deletes history or grants
  authority.
- Keep compaction, durable memory, source acceptance, folder trust, and run
  approvals as separate typed state.
- Let Plume propose memories only from explicit corrections, preferences,
  repeated stable workflow choices, or direct remember requests. Durable
  writes require user approval and retain provenance, scope, revision,
  correction, and forget behaviour.
- Preserve legacy local/project sessions non-destructively while the new Home
  path lands. Import is explicit and never rewrites or deletes the source.
- Treat Plume as a general-purpose local harness rather than a coding-only
  editor. Documents, decks, and spreadsheets are first-class outcomes, so
  binary and generated artefacts get a guarded whole-file write under the same
  approval gate instead of a widened patch path. The consumer surface stays
  plain: the user states an outcome and the harness does the work.

**Commissioned sequence:**

1. Contract and evaluation fixtures that separate history, projection,
   compaction, memory, grants, and run authority.
2. Durable app-private Home conversation and relaunch restoration.
3. Provider-neutral compaction with review/rebuild and repeated-cycle tests.
4. Typed reviewable learning, initially without ambient prompt insertion.
5. Opaque read-only multi-folder grants through existing path, size, binary,
   hardlink, redaction, and exact-manifest gates.
6. Chat-first shell, no normal Projects surface, and legacy-session access.
7. One-writable-folder run leases, read-only reference folders, approval
   previews, budgets, cancellation, traces, and guarded verifier execution.
8. Bounded multi-iteration read/edit/test/fix execution.
9. Real task-pipeline model validation and evidence-backed catalogue updates.

**Dependencies:** Existing session persistence, app-private and project memory,
trusted path resolution, exact context manifests, patch checkpoints, agent
controller scaffold, provider cancellation, and native folder selection.

**Acceptance boundary:** A user can keep one conversation across relaunches,
multiple safe compactions, approved memory corrections, and tasks involving
several explicitly granted folders. A run can modify only its one approved
working folder, reference folders remain read-only, and every prompt/action can
be reconstructed from exact structured evidence.

**Non-goals:** A home-directory grant, multiple writable roots in one run,
automatic disk-wide project discovery, opaque provider compaction as the only
record, silent learning, semantic retrieval, arbitrary tool execution, or
implied Browser/macOS control.

## Track: Documentation And Agent Navigation

**Outcome:** Humans and agents can reach current product, implementation,
roadmap, research, safety, and history truth without treating chronology as
current status.

**Current floor:** The repository has detailed product and domain contracts,
plus this task-oriented map and ordered roadmap. The feature inventory is the
only repository-wide status authority.

**Dependencies:** A seeded feature inventory, research and history indexes, and
repository-relative links that verification can check.

**Next deliverable:** Keep `README.md` → `docs/README.md` → feature
inventory/roadmap → domain maps aligned as product surfaces change. Keep
chronological implementation evidence in `docs/history/`, never in the agent
entrypoint.

**Non-goals:** Product behavior changes, deleting historical evidence, or
assigning slice numbers to uncommissioned ideas.

## Track: Library And Second Brain

**Outcome:** A calm Library makes app-private user memory, trusted-project
memory, curated topics, exact backlinks, provenance, and lexical search visible
without changing prompt authority.

**Current floor:** Library is available with or without a project. **About you**
reads app-private memory; **This project** and **Topics** appear only for the
currently trusted project. Its independently loaded sources fail and retry
without blanking healthy neighbors, and identity checks stop stale responses
from repainting another project. Search stays within the selected visible
scope. Exact stored links and backlinks remain organization metadata only: they
never place prompt context. User memory is never ambient and reaches a prompt
only through an explicit `userMemoryEntry` shelf ref.

**Dependencies:** A backend-owned app-data user-memory store, trusted bounded
project reads for project memory/topics, exact canonical topic references, and
partial-failure handling that keeps each healthy source useful.

**Next deliverable:** The reviewable-learning phase may propose exact memory
changes with user approval, provenance, correction, and forget. Ambient use
remains off until a separate exact-manifest acceptance slice earns it.

**Non-goals:** Semantic retrieval, background dreaming, automatic topic
generation, silent cross-folder aggregation, or treating links/backlinks as
prompt-selection authority.

## Track: Explicit Context Placement And Linked Work

**Outcome:** Users can place visible, provenance-bearing sources onto one chat
or agent target and see exactly what will be resolved at send time.

**Current floor:** Project chats own a sticky ordered shelf of opaque typed
references for project files or line selections, exact memory entries, and
canonical curated topic files, plus immutable user-captured Browser selections
or visible page text. The backend re-resolves every ref at preview and
send through its owning trust/path/redaction gates; send is all-or-nothing and
returns the exact accepted manifest. A local session may hold only app-private
`userMemoryEntry` and owned Browser evidence; project files, project memory, and
topics remain project-only. The shelf persists only with its owning session,
while fork/rewind children start empty and retain historical accepted-turn
manifests. Library objects and the eligible Files inspector action can be
clicked or dragged into the temporary chat drop target; the gesture adds the
same opaque ref and reveals the canonical shelf. Links and backlinks remain
organization metadata and never populate the shelf.

**Dependencies:** The Library workspace, typed source references, owning
backend resolvers, project/session scoping, and all-or-nothing preview/send
parity.

**Next deliverable:** Preserve exact preview/send/persistence parity as future
explicit source kinds are separately commissioned.

**Non-goals:** Frontend-supplied prompt text, silent ambient retrieval,
cross-project context, or links that add context by themselves.

## Track: Sandboxed Browser And Evidence Capture

**Outcome:** A first-class Browser workspace can navigate and capture explicit
evidence without giving remote content Plume command or IPC authority.

**Current floor:** Every persisted local or project chat owns an integrated
WebKit Browser workspace with bounded tabs, admitted top-level history, and a
persisted split or expanded layout. The trusted `main` webview receives an
explicit generated application-command allowlist, while each separately
labelled `browser-sandbox` webview matches no Tauri capability. Top-level
non-HTTP(S) navigation, embedded credentials, URLs over 8 KiB, popups, and
downloads are blocked. Generation, exact-URL, session-owner, and project/trust
checks reject stale navigation and capture callbacks. Browser exposes visible
tabs, address, Back, Forward, Reload, Attach, and layout controls. A human can
explicitly capture selected text, readable page text, or the visible viewport
as an immutable bounded record owned by that chat; the context shelf carries
only its opaque id. Screenshot bytes reach only fixed Qwen2-VL through MLX-VLM or
an exact Ollama model freshly verified as vision-capable; text-only and
unverifiable models fail closed.

**Dependencies:** Main-window versus child-webview capability isolation,
localhost policy, bounded navigation state, and an explicit evidence-attachment
contract.

**Next deliverable:** Keep human browsing and evidence capture separate from
the later guarded agent-action executor. Agent clicks still come later. A
nonblocking hardening candidate is a Rust-owned activation epoch/token checked
by deactivate and suspend commands, fencing theoretically late same-session
native commands after frontend deadlines. This is not a reproduced production
bug and is not commissioned implementation.

**Non-goals:** Agent-driven browser actions in the first slice, arbitrary
remote-page privileges, hidden browsing, or macOS host control.

## Track: Safe Coding-Agent Execution

**Bounded research-note milestone:** The Stage A implementation can turn up to
10 exact Browser text captures already attached to one chat into a cited
Markdown research note through Apple On-Device, fixed Qwen Coder, or fixed
Qwen2-VL. Qwen2-VL may additionally inspect exact owner-shelf Browser screenshot
PNGs, but at least one text capture remains required for citation provenance.
It has two internal text-protocol submit actions, 13 logical turns, at most 26
provider calls, visible progress/Stop, immutable session-local versions, and
explicit native export. It does not search, fetch, navigate, execute general
tools, read arbitrary files, or inherit memory/topic/link authority. Automated evidence and
the normal packaged Apple/Qwen, recovery, Stop, review-needed, persistence, and
export paths are recorded. The feature remains `partial`: the exact-head Qwen2-VL
packaged research/export matrix is not yet recorded, context-overflow and stale-owner
fault injection are deterministic test evidence, and Stage A still requires
exact attached Browser text and produces Markdown. Stage B network
access and Stage C search remain separately reviewed candidates, not implied
follow-ons.

**Outcome:** Plume can run a bounded read/edit/test/fix loop while every read,
patch, command, approval, and failure remains scoped and visible.

**Current floor:** A patch-only single-step path can propose a diff for explicit
user apply/revert, with typed events and approval/config foundations. The
separate research-note controller produces an inert Markdown artifact and does
not broaden coding-agent authority. A bounded coding loop and broad
command/tool executor are not shipped.

**Dependencies:** Explicit context placement, the existing trust and patch
boundaries, a guarded executor, per-project approvals, cancellation, and
verifier result capture.

**Next deliverable:** After one-writable-folder run leases ship, connect one
deeper guarded loop slice that proves bounded read/edit/test/fix progress
without bypassing patch or command approval gates.

**Non-goals:** Unapproved shell execution, arbitrary `tools.invoke`, invisible
background mutation, or claiming multi-agent execution.

## Track: Skills, Tools, Plugins, And External Agents

**Outcome:** Procedural skills and progressively disclosed tools extend the
agent through explicit, inspectable contracts instead of ambient authority.

**Current floor:** Project skills and session-to-skill drafting are shipped.
The tool catalog supports discovery foundations, but broad external tool, MCP,
plugin, and multi-agent execution are not shipped.

**Dependencies:** The guarded coding-agent executor, capability-specific
approval policy, bounded results, provenance, and lifecycle cleanup.

**Next deliverable:** Commission one executor-backed, allowlisted tool path only
after the core loop can expose its request, approval, result, and failure in the
same visible run history.

**Non-goals:** Cloning another agent platform's breadth, automatic skill
creation, unrestricted plugin authority, or unsupervised multi-agent work.

## Track: Operability, Safety, Observability, And Computer Use

**Outcome:** Humans, assistive technology, and external automation can inspect
and control Plume, while Plume's own emitted actions stay session-scoped,
approved, stoppable, and auditable.

**Current floor:** Plume's visible UI supports the receiving role for external
computer-use agents. Computer-use emission is researched but not shipped.

**Dependencies:** Browser Phase A, guarded execution, target allowlists,
foreground approval, an append-only visible trace, and Pause/Stop controls.

**Next deliverable:** After the Browser and coding-loop gates, emit the first
bounded `computer.*` action only inside the named sandbox session.

**Non-goals:** Persistent blanket permission, hidden actions, conflating the
receiving and emitting roles, or Phase B macOS host control in Phase A.

## Track: Local Models, Benchmarks, And Launch Readiness

**Outcome:** Plume owns an honest MLX-first local path and publishes model or
product claims only from reproducible evidence tied to hardware and commits.

**Current floor:** Plume-managed MLX-LM/MLX-VLM is the happy path. The current
source candidate carries a verified bundled runtime, while fixed Qwen Coder and
Qwen2-VL weights are explicit, resumable downloads into app data. The
top-bar catalog can start and select either model without project trust; the
existing v0.1.0 public artifact remains the earlier Qwen-era release. Apple's
separately bundled on-device
adapter is also selectable when the host framework reports it available;
availability is host state, not a universal macOS promise, and no Private Cloud
Compute path exists. Ollama and other local runtimes remain compatibility
paths. These are chat providers, not the unshipped deeper coding-agent loop.
The benchmark harness, presets, and read-only evidence viewer are shipped.

**Dependencies:** The target 128 GB M5 Max, verified model artifacts and runtime
identity, deterministic fixtures, raw records, generated summaries, and Plume
commit provenance.

**Next deliverable:** Keep the current fixed models stable while the guarded
task pipeline lands. Then validate Qwen3.8-27B against real Plume fixtures
before considering catalogue support. Muse Glimmer is a challenger only;
Qwen3.8-Flash-Next is not a practical catalogue target at its current MLX
footprint. Any runtime update remains an explicit pinned, hash-verified,
packaged and cancellation-tested slice. Run the broader target-hardware matrix
when the hardware exists, then use recorded results for the evidence-backed
D130 launch rewrite.

**Non-goals:** Performance claims before measured records, presenting a runtime
as model weights, silent or arbitrary downloads, making Ollama the default,
claiming Apple availability on every host, or treating chat-provider onboarding
as broad tool execution.
