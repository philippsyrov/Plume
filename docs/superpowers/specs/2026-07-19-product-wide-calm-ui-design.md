# Product-Wide Calm UI Design

**Date:** 2026-07-19

**Status:** Approved; foundations candidate implemented, screen slices pending

**Base:** `origin/main@8f5903525fac4776ec83fbe190301285d651e9f0`

## Goal

Make every currently shipped Plume surface feel like one restrained, dependable
Mac product before final screenshots and Build Week packaging. The cleanup must
reduce visible explanation, normalize typography and controls, and give each
screen one obvious next action without weakening trust, provenance, provider,
or accessibility boundaries.

This is an interface and information-hierarchy campaign. It does not add model
authority, tools, retrieval, Browser control, shell execution, or new product
capabilities.

## Product standard

“Apple-clean” means restraint and coherence, not copying Apple artwork or
making Plume look like a system preference pane.

Plume should follow these rules everywhere:

1. One primary action per state. Secondary actions stay quiet; destructive and
   expert actions appear only where they are needed.
2. Explain a fact once. Prefer a short label plus optional details over
   permanent paragraphs of instructional copy.
3. Use whitespace and type hierarchy before borders. Borders identify true
   controls, selected items, or major regions rather than wrapping every group.
4. Use ordinary language in the default path. Provider diagnostics, paths,
   commands, prompt manifests, and implementation details remain available
   through explicit disclosure where they are required.
5. Preserve Plume's warm paper-and-ink identity. Do not introduce gradients,
   glossy cards, novelty decoration, emoji controls, or a competing icon set.
6. Keep the local model lightweight enough that inference receives most of the
   machine's memory. Visual polish must not add a heavy frontend runtime,
   animation framework, or new UI dependency.

## Foundations

### Typography

- Use the existing macOS system stack for all interface and prose text.
- Reserve the monospace stack for code, paths, commands, technical identifiers,
  and measured benchmark values.
- Define a small semantic scale: window title, page title, section title, body,
  secondary body, and compact metadata. Avoid one-off font sizes.
- Use weight and colour consistently. Italics are not a substitute for muted
  status text and should disappear from routine UI copy.
- Keep body copy at a readable measure. Long help or policy text belongs in a
  dedicated reading surface, not a narrow control panel.

### Spacing and geometry

- Use the existing spacing tokens and normalize recurring panel padding,
  section gaps, row heights, and control heights.
- Use one small radius for controls and one soft radius for floating surfaces.
- Keep buttons large enough for reliable pointer use and preserve visible focus.
- Major empty states sit near the visual centre of their working area without
  turning into oversized marketing cards.

### Controls and surfaces

- Primary buttons are reserved for the single next action.
- Secondary buttons use a quiet outline or text treatment.
- Destructive controls are never the most visually prominent action in a list.
- Menus, popovers, drawers, and modals share one surface, shadow, border, title,
  dismissal, and focus-restoration contract.
- Status chips are used only when the status helps the user decide what to do.
  Decorative `open`, `ready`, or count chips should become plain metadata.

### Motion and colour

- Motion explains a state change; it is short and respects reduced motion.
- Light and dark themes preserve the same hierarchy. Secondary text and borders
  must remain distinguishable without making the whole screen high contrast.
- Existing semantic success, warning, and error colours retain their meaning.

## Information architecture

The primary workspace structure stays intact: Chat, Library, Files, Browser,
and Benchmarks remain the current destinations. The cleanup does not introduce
a new global navigation model.

Settings changes from one long mixed modal into a stable category layout:

- **General:** appearance and ordinary application preferences.
- **Models:** Apple On-Device, Qwen, Ollama compatibility, downloads, and
  provider status.
- **Personal:** app-private About you memory.
- **Project:** trusted-project memory and project instructions, available only
  when the current project boundary permits them.
- **Advanced:** tools, diagnostics, raw paths, and expert-only controls.

At normal window sizes, categories appear in a compact sidebar with one scroll
region for the selected page. At narrow sizes they may become a select or
stacked category control. Changing presentation must not change ownership,
persistence, or IPC authority.

## Surface contracts

### Project trust and first run

The trust screen becomes a focused decision rather than a sparse diagnostic
page. It shows the project name and shortened path, one plain explanation of
what trust enables, and two clear actions: **Trust and open** and **Cancel**.
Detailed safety boundaries remain behind **What does trust allow?**

Trust is never preselected, implied, or weakened. Keyboard access, visible
focus, cancellation, and the exact trusted-root decision remain unchanged.

The no-project state offers one primary **Open project** action and a quiet path
to local chat. It does not repeat setup instructions in multiple cards.

### Chat

Empty chat keeps a short prompt and one context-aware primary action. When no
model is selected, **Choose a model** is shown once; the disabled composer may
state the same requirement only through its placeholder or accessible reason,
not an additional visible paragraph.

The transcript retains exact messages, accepted context manifests,
attachments, research progress, citations, errors, patch previews, Apply,
revert, stop, copy, and runtime metadata. Those elements become visually
layered:

- answer content first;
- user-visible evidence and actions second;
- runtime and provenance diagnostics under disclosure when they are not needed
  to understand the answer.

Research footnote markers render as readable references in inert preview. The
preview remains inert and provenance-only citation validation remains exactly
as shipped.

### Model chooser

Keep the existing compact-row direction. The default view shows model name,
one suitability line, current availability, and one action. License, source,
hash, runtime errors, and diagnostics remain under **Details** unless they block
selection or require user action.

Apple On-Device remains availability-gated. Qwen remains the fixed bundled
MLX-LM path. Ollama remains compatibility rather than the default centre of the
experience.

### Settings

Each category page starts with a title and, only where helpful, one short
sentence. Related controls use aligned rows and restrained section dividers.
Avoid cards inside cards and avoid repeating scope explanations beneath every
control.

Provider failures stay visible beside the affected provider and keep Retry or
Cancel actions. Advanced disclosure cannot hide an error that blocks the
current task.

Project memory forms use a consistent label-field-help rhythm. Technical IDs,
paths, and raw tool configuration move to Advanced unless they are necessary
for an explicit project-memory action.

### Library

Keep the source rail and reading canvas. Reduce unavailable labels, repeated
scope prose, and decorative counts. The overview should answer two questions:
what is saved about the user, and what belongs to this trusted project?

Memory links and backlinks remain organization metadata only. Nothing in this
redesign implies semantic retrieval, automatic prompt selection, dreaming, or
cross-project aggregation.

### Files

Keep the current tree-and-editor layout. Normalize header, tree row, metadata,
and editor typography. File type, size, and path details stay secondary. Empty
space remains working space rather than being filled with explanatory cards.

No frontend filesystem authority is added. Rust continues to resolve every
source and write through the existing guarded boundaries.

### Browser

Keep the human-controlled per-chat split workspace. The task column becomes
more compact and removes repeated explanation. Current-file context is shown
as a quiet attachment state rather than a large instructional block.

The redesign must not imply agent navigation, Plume-emitted computer actions,
or macOS host control. Browser loading and evidence states remain explicit.

### Benchmarks

Replace the raw command-and-path empty page with a calm **No benchmark results
yet** state and one appropriate next action. Setup commands, evidence paths,
hardware details, and benchmark methodology remain available through **How to
run a benchmark** or an equivalent disclosure.

Recorded results retain exact hardware, model, runtime, fixture, result, and
Plume commit evidence. Visual simplification cannot weaken that contract.

### Help

Replace the loud card grid with a compact, scannable list of common tasks and a
single **Open handbook** action. Keep safety and recovery information easy to
find without presenting the entire handbook inside the modal.

### Workspace views and session management

The workspace drawer keeps only reachable destinations. Remove future or
disabled promotional rows such as **Terminal soon** from ordinary navigation.
Use selection and hierarchy instead of `open` chips.

Archived chats show name, scope, and useful date information. **Unarchive** is
the row action; **Delete** moves into a secondary menu or confirmation flow so
it is not repeated as a prominent button on every row. Rename, continue,
rewind, archive, and delete retain their present behaviour and safeguards.

## Copy rules

- Titles name the place: **Settings**, **Models**, **Library**.
- Buttons name the action: **Open project**, **Choose model**, **Trust and
  open**.
- Helper text explains consequences, not labels the user can already see.
- Avoid internal terms such as harness, manifest, provider-neutral, runtime
  supervisor, IPC, or artifact publication in the default path.
- Do not hide required trust, destructive-action, download-size, or provenance
  information merely to reduce text.
- Error messages say what happened and what the user can do next. Diagnostics
  can follow under disclosure.

## Accessibility and responsive behaviour

- Preserve existing accessible names and add stable names where controls are
  currently identified only visually.
- Every dialog and popover traps focus where appropriate, closes with Escape,
  and restores focus to its trigger.
- All primary workflows work by keyboard at the minimum supported window size.
- Do not rely on colour alone for selection, progress, warning, or error state.
- Verify contrast with measurement in both themes; screenshots alone are not
  sufficient evidence.
- Preserve reduced-motion handling and live-region semantics for downloads,
  streaming, research progress, errors, and completion.

## Delivery slices

The campaign should land as small PRs so visual work does not obscure behaviour
regressions:

1. **Foundations:** semantic type, spacing, surface, control, focus, and theme
   rules plus shared primitives where existing duplication justifies them.
2. **First-run and Chat:** project trust, no-project and empty-chat states,
   composer hierarchy, transcript, research preview, and model chooser.
3. **Settings:** category navigation, provider pages, personal/project memory,
   and Advanced disclosure.
4. **Knowledge and guidance:** Library, Help, Benchmarks, and honest empty
   states.
5. **Workspace:** Files, Browser task column, workspace drawer, and session
   management.
6. **Final visual QA:** light/dark contrast, keyboard traversal, narrow-window
   containment, exact-viewport before/after comparison, packaged smoke, and
   copy consistency.

Each slice starts with failing behavioural or structural tests where behaviour
can regress. Pure CSS changes receive focused DOM/CSS guardrails plus packaged
visual comparison. No slice adds a dependency without separate approval.

## Verification

Every implementation PR must run focused frontend tests, TypeScript, the full
relevant suite, and the repository verifier. The final exact head must also
receive:

- `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`;
- pre-commit and gitleaks;
- packaged-app smoke for every surface listed above;
- light and dark screenshots at a consistent viewport;
- keyboard traversal and focus-return checks;
- minimum-window containment checks;
- a findings-only exact-head review;
- GitHub verify and gitleaks before squash merge.

## README and efficiency claims

Final README screenshots must come from the polished exact packaged build, not
the current audit build or a mockup.

Plume may explain that it uses Tauri and the system WebView rather than shipping
an Electron runtime. It must not claim lower memory use, fewer logical writes,
less NAND wear, or better efficiency than Claude, ChatGPT, or another product
until a repeatable Plume measurement records comparable workload, duration,
hardware, process boundaries, logical writes, and physical-device evidence.

The renderer experiment is motivation and a benchmark hypothesis, not product
proof. Any eventual README claim must link to the recorded methodology and use
the narrowest wording supported by the result.

## Non-goals

- A new visual brand, logo, icon family, or marketing illustration.
- New navigation destinations or unreachable roadmap controls.
- Model/provider changes, downloads, or runtime work.
- Broader coding-agent execution, arbitrary tools, shell, or patch authority.
- Search, URL fetch, agent Browser actions, computer-use emission, or host
  control.
- Semantic retrieval, automatic topics, dreaming, or link-based prompt
  authority.
- README screenshots or public release publication before exact-build QA.

## Completion criteria

The campaign is complete when a new user can open or trust a project, choose an
available model, start a chat, understand attached context, navigate every
shipped workspace, and recover from ordinary errors without reading internal
implementation language. The same hierarchy must hold in light and dark mode,
at normal and minimum window sizes, by pointer and keyboard, while all current
authority and provenance contracts remain intact.
