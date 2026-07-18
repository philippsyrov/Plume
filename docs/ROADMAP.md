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
7. Deeper guarded coding-agent execution.
8. Computer-use emission inside the sandbox.

The 128 GB M5 Max benchmark matrix runs when the hardware exists. The D130
launch rewrite follows measured evidence and does not block unrelated product
work.

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

**Next deliverable:** No automatic retrieval slice is commissioned. A later
retrieval-preview milestone must earn authority through explicit evaluation.

**Non-goals:** Semantic retrieval, background dreaming, automatic topic
generation, cross-project aggregation, or treating links/backlinks as
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
only its opaque id. Screenshot bytes reach only an exact Ollama model freshly
verified as vision-capable, while MLX and unverifiable models fail closed.

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

**Outcome:** Plume can run a bounded read/edit/test/fix loop while every read,
patch, command, approval, and failure remains scoped and visible.

**Current floor:** A patch-only single-step path can propose a diff for explicit
user apply/revert, with typed events and approval/config foundations. A bounded
multi-turn execution loop and broad command/tool executor are not shipped.

**Dependencies:** Explicit context placement, the existing trust and patch
boundaries, a guarded executor, per-project approvals, cancellation, and
verifier result capture.

**Next deliverable:** One deeper guarded loop slice that proves bounded
read/edit/test/fix progress without bypassing patch or command approval gates.

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

**Current floor:** Plume-managed MLX-LM is the happy path; releases carry a
verified bundled runtime while the fixed Apache-2.0 Qwen Coder weights are an
explicit, resumable download into app data. The top-bar catalog can start and
select that model without project trust. Apple's separately bundled on-device
adapter is also selectable when the host framework reports it available;
availability is host state, not a universal macOS promise, and no Private Cloud
Compute path exists. Ollama and other local runtimes remain compatibility
paths. These are chat providers, not the unshipped deeper coding-agent loop.
The benchmark harness, presets, and read-only evidence viewer are shipped.

**Dependencies:** The target 128 GB M5 Max, verified model artifacts and runtime
identity, deterministic fixtures, raw records, generated summaries, and Plume
commit provenance.

**Next deliverable:** Complete exact-head packaged smoke for Apple availability
and Qwen download/start/relaunch, then run the full benchmark matrix when the
target hardware exists and use recorded results for the evidence-backed D130
launch rewrite.

**Non-goals:** Performance claims before measured records, presenting a runtime
as model weights, silent or arbitrary downloads, making Ollama the default,
claiming Apple availability on every host, or treating chat-provider onboarding
as broad tool execution.
