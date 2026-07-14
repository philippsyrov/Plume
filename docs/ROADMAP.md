# Plume Roadmap

Status vocabulary comes from [FEATURE_INVENTORY.md](FEATURE_INVENTORY.md).
Research is not implementation. Slice numbers are assigned only when a slice is
commissioned.

## Commissioned Sequence

1. Documentation and agent navigation — shipped.
2. Typed explicit context shelf with manual Use in chat — shipped.
3. Drag/drop convenience for typed context sources — shipped.
4. Browser remote-content capability isolation — shipped.
5. Human-controlled Browser workspace — next.
6. Bounded Browser evidence placement.
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

**Next deliverable:** Complete the navigation spine with the inventory,
research/history entry points, and Markdown-link and inventory checks.

**Non-goals:** Product behavior changes, deleting historical evidence, or
assigning slice numbers to uncommissioned ideas.

## Track: Project Knowledge And Second Brain

**Outcome:** A calm project knowledge workspace makes memories, curated topics,
backlinks, provenance, and lexical search visible in one place.

**Current floor:** The trusted-project Knowledge workspace provides capped
Markdown topic navigation, exact-ref memory backlinks, unlinked and stale-linked
views, provenance, and lexical memory-text search. Its two bounded sources fail
and retry independently, and stale responses cannot repaint another project.
Topic links remain organization metadata only: they do not place prompt context.

**Dependencies:** Trusted bounded reads for memory and topics, exact canonical
topic references, and partial-failure handling that keeps either source useful
when the other fails.

**Next deliverable:** No automatic retrieval slice is commissioned. A later
retrieval-preview milestone must earn authority through explicit evaluation.

**Non-goals:** Semantic retrieval, background dreaming, automatic topic
generation, or treating topic links as prompt-selection authority.

## Track: Explicit Context Placement And Linked Work

**Outcome:** Users can place visible, provenance-bearing sources onto one chat
or agent target and see exactly what will be resolved at send time.

**Current floor:** Project chats own a sticky ordered shelf of opaque typed
references for project files or line selections, exact memory entries, and
canonical curated topic files. The backend re-resolves every ref at preview and
send through its owning trust/path/redaction gates; send is all-or-nothing and
returns the exact accepted manifest. The shelf persists only with its project
session, while fork/rewind children start empty and retain historical accepted
turn manifests. Knowledge memory/topic cards and the eligible Files inspector
action can now be dragged into a temporary **Drop into project chat** target;
the gesture adds the same opaque ref and reveals the canonical shelf. Topic
links remain organization metadata and never populate the shelf.

**Dependencies:** The Knowledge workspace, typed source references, owning
backend resolvers, project/session scoping, and all-or-nothing preview/send
parity.

**Next deliverable:** Extend the same resolver and manifest contract to bounded
Browser evidence only after the Browser owns a safe capture format.

**Non-goals:** Frontend-supplied prompt text, silent ambient retrieval,
cross-project context, or links that add context by themselves.

## Track: Sandboxed Browser And Evidence Capture

**Outcome:** A first-class Browser workspace can navigate and capture explicit
evidence without giving remote content Plume command or IPC authority.

**Current floor:** The trusted `main` webview now receives an explicit generated
application-command allowlist, while the separately labelled
`browser-sandbox` webview matches no Tauri capability. Three trusted-main-only
backend commands create, close, and inspect one incognito HTTP(S) sandbox
window. Top-level non-HTTP(S) navigation, credentials, popups, and downloads
are blocked; direct `MockRuntime` tests prove the sandbox cannot invoke `ping`
or main's event-listener command. No normal-user Browser navigation surface or
evidence capture is shipped yet.

**Dependencies:** Main-window versus child-webview capability isolation,
localhost policy, bounded navigation state, and an explicit evidence-attachment
contract.

**Next deliverable:** A calm human-controlled Browser workspace with visible
URL, title, loading/error state, back/forward/reload controls, explicit
localhost transitions, and packaged hostile-page smoke. Agent clicks come
later.

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

**Current floor:** Plume-managed MLX-LM is the happy path; Ollama and other
local runtimes are compatibility paths. The benchmark harness, catalog,
presets, and read-only evidence viewer are shipped.

**Dependencies:** The target 128 GB M5 Max, verified model artifacts and runtime
identity, deterministic fixtures, raw records, generated summaries, and Plume
commit provenance.

**Next deliverable:** Run the full benchmark matrix when the target hardware
exists, then use those recorded results for the evidence-backed D130 launch
rewrite.

**Non-goals:** Performance claims before measured records, presenting a fake
runtime as a model, making Ollama the default, or blocking unrelated product
work on unavailable hardware.
