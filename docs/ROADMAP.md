# Plume Roadmap

Status vocabulary comes from [FEATURE_INVENTORY.md](FEATURE_INVENTORY.md).
Research is not implementation. Slice numbers are assigned only when a slice is
commissioned.

## Commissioned Sequence

1. Documentation and agent navigation.
2. Knowledge workspace and backlinks.
3. Explicit context shelf and drag/drop.
4. Sandboxed Browser Phase A.
5. Deeper guarded coding-agent execution.
6. Computer-use emission inside the sandbox.

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

**Current floor:** Project memory CRUD, bounded text search, curated topic
files, exact prompt-context manifests, audited duplicate distillation, and
user-managed memory-to-topic links are shipped. Topic links are organization
metadata only.

**Dependencies:** Trusted bounded reads for memory and topics, exact canonical
topic references, and partial-failure handling that keeps either source useful
when the other fails.

**Next deliverable:** A dedicated read-only Knowledge workspace with topic
navigation, exact-ref backlinks, unlinked memories, provenance, and lexical
search.

**Non-goals:** Semantic retrieval, background dreaming, automatic topic
generation, or treating topic links as prompt-selection authority.

## Track: Explicit Context Placement And Linked Work

**Outcome:** Users can place visible, provenance-bearing sources onto one chat
or agent target and see exactly what will be resolved at send time.

**Current floor:** A trusted project file or line selection can be attached to
chat, and exact context manifests report accepted memory entries and topic
files. There is no general context shelf or drag/drop contract.

**Dependencies:** The Knowledge workspace, typed source references, owning
backend resolvers, project/session scoping, and all-or-nothing preview/send
parity.

**Next deliverable:** A typed context-source contract and visible shelf for
project files or selections, memory entries, and curated topic files, with
`Use in chat` before drag/drop convenience.

**Non-goals:** Frontend-supplied prompt text, silent ambient retrieval,
cross-project context, or links that add context by themselves.

## Track: Sandboxed Browser And Evidence Capture

**Outcome:** A first-class Browser workspace can navigate and capture explicit
evidence without giving remote content Plume command or IPC authority.

**Current floor:** Browser workspace and optional browser-tool descriptions are
non-executing surfaces. Browser execution and evidence capture are not shipped.

**Dependencies:** Main-window versus child-webview capability isolation,
localhost policy, bounded navigation state, and an explicit evidence-attachment
contract.

**Next deliverable:** A capability-isolation proof showing that a dedicated
remote-content webview inherits no Plume IPC or command authority; agent clicks
come later.

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
