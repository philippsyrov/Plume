# Plume Project Spec

## 1. Short Version

Plume is a local-first AI coding editor for laptops, especially Apple Silicon Macs with limited unified memory. The product should feel like a quiet hand-drawn cafe for coding: white surfaces, black scribbled outlines, paper-like panels, and a calm editor layout. Under the cute skin, it should be serious about local model performance, memory limits, file safety, and a Codex-style coding workflow.

The basic idea:

- A lightweight desktop app, not Electron-first.
- A real code editor surface, not just a chat window.
- A local model manager that can run Gemma, Qwen, DeepSeek-style coder models, and other open models through swappable providers.
- A small but honest coding agent that can read files, propose diffs, edit scoped files, run commands, and show results.
- A resource-aware design that survives on a 16 GB Mac instead of pretending every laptop is a workstation.

The working name is **Plume**. It is a prototype/open-source project name, not a final trademark promise. If it becomes public and gains users, the name can mutate later.

## 2. Why Plume Exists

Most local AI tools split into two awkward worlds.

First world: local chat apps. They run models locally, but they are mostly chat boxes. They can help explain code, but they do not feel like a project-aware coding partner.

Second world: full coding agents like Codex, Claude Code, Cursor, or similar tools. These have the right workflow: read the repo, edit files, run tests, inspect diffs, and iterate. The problem is that they usually depend on very strong cloud models, or they push local models through runtimes that may not fit well on smaller machines.

Plume is meant to sit between those worlds.

It should be a local coding workspace where the model is treated like an engine. Some engines are tiny and fast. Some are larger and cleverer. Some run through MLX on Mac. Some run through Ollama, LM Studio, or llama.cpp. The UI and agent loop should adapt to the engine instead of assuming every model can handle huge repo context and open-ended autonomy.

The stronger north-star version is: Plume should become a local Hermes-class coding agent for open weights. Hermes proves that persistent memory, skills, toolsets, and long-running agent work matter; Plume's angle is to bring that class of capability into a native local coding editor with MLX-first model ownership, visible diff/apply/revert safety, and no default cloud dependency. See `docs/LOCAL_AGENT_NORTH_STAR.md`.

The product promise is not:

> Run giant frontier coding agents on any laptop.

The honest promise is:

> Make local coding with open models feel calm, useful, private, and resource-aware.

## 3. Visual Identity

The visual reference is a black-and-white hand-drawn cafe wall:

- White base.
- Black ink outlines.
- Slightly imperfect scribbled borders.
- Drawn shelves, cups, drawers, panels, and small decorative lines.
- Cozy but not childish.
- Paper and ink rather than glass and gradients.

The app should not look like a generic dark developer dashboard. It should look like a notebook cafe where code happens.

Visual rules:

- Mostly white background.
- Black or near-black outlines.
- Thin hand-drawn borders for panels.
- Occasional gray shading, hatching, or sketch texture.
- Minimal accent colors, used only for state.
- Red can mean destructive or failing.
- Green can mean passing.
- Amber can mean warning or model memory pressure.
- No giant gradients.
- No glossy SaaS cards.
- No purple-blue AI blobs.
- No fake futuristic chrome.

The design should still be usable for real coding. The hand-drawn style should decorate the frame, not damage readability.

Important UI balance:

- Code text must stay crisp and readable.
- The editor itself can be clean monospace.
- Scribbled outlines belong more around panes, tabs, buttons, model badges, and empty states.
- Diff views must be boring enough to trust.
- Terminal output must be plain and legible.

## 4. Name

Working name: **Plume**.

Why it fits:

- It means feather or pen-like writing tool.
- It matches ink, paper, and hand-drawn UI.
- It feels lighter than names like "Zen Coder".
- It does not scream AI.
- It leaves room for a calm brand.

Possible future variants:

- Plume Code
- Black Plume
- Plume Studio
- Plume Cafe
- Plume Local

For now, use **Plume** everywhere in docs and package naming unless there is a real reason to rename.

## 5. Target User

Primary user:

- CS student.
- Indie hacker.
- Mac user.
- Wants local AI help without burning cloud credits.
- Has a 16 GB Apple Silicon laptop.
- Likes tools that are understandable, tweakable, and honest about limits.

Secondary users:

- Developers experimenting with open models.
- Privacy-focused coders.
- Students doing coursework who want a local assistant.
- Hackers building small web apps, scripts, notebooks, and prototypes.

Plume should not initially target huge enterprise repos, giant monorepos, or full replacement of Cursor/Codex/Claude Code.

## 6. Core Product Shape

Plume should start as a thin local coding workspace.

Main panes:

- Left: project files.
- Middle: code editor and diff viewer.
- Right: local AI chat or agent panel.
- Bottom: terminal, tests, logs, and model runtime output.
- Top or bottom status strip: model, context, memory pressure, provider, git state.

The first screen should be the working app, not a landing page. When opened, Plume should let the user open a folder, choose a local model provider, and start a coding session.

The experience should feel like:

1. Open a project folder.
2. Plume reads project instructions like `AGENTS.md`.
3. Pick a local model.
4. Ask for a scoped change.
5. Plume shows a plan.
6. Plume proposes or applies diffs.
7. Plume runs verification if allowed.
8. Plume summarizes exactly what changed.

## 7. Product Principles

### 7.1 Local First

The default path should use local models and local project files. Cloud calls should be optional, explicit, and disabled by default.

### 7.2 Resource Honesty

Plume should tell the user when a model is too large, context is too high, memory is risky, or a task is too broad for the selected model.

Bad behavior:

- Load a giant model and freeze the machine.
- Hide memory usage.
- Pretend a small model can refactor a huge codebase safely.

Good behavior:

- "This model is good for small edits."
- "This context size may push a 16 GB Mac into memory pressure."
- "Use a smaller context or a smaller model."

### 7.3 Scope Before Autonomy

Local models should not be pushed into huge vague tasks by default. The app should prefer scoped edits, clear plans, and diffs the user can inspect.

### 7.4 Beautiful But Useful

The hand-drawn style is part of the product identity, but coding clarity wins. The UI should be charming around the edges and strict in the editor, diff, and terminal.

### 7.5 Model Agnostic

Plume should not be tied to one model family. Gemma, Qwen, DeepSeek-style coder models, Phi, Llama, and future models should fit through a provider abstraction.

### 7.6 Open Source Friendly

The codebase should be easy to read, hack, and extend. Config should be visible. Model provider adapters should be simple.

### 7.7 Simple Mode vs Developer Mode

Plume's trusted-project shell ships today as a dense three-zone
layout: file tree + provider strip on the left, agent workspace
in the middle, file inspector on the right, status strip on top.
That surface is right for a developer who already knows what
Plume is. It is not the right first impression for the target
user in § 5 — a CS student, an indie hacker, a first-time
local-LLM user. The moment a project is trusted, the calm cafe
identity gets buried under chips and panels that look like an
IDE dashboard.

The product resolves this with a UX axis, **not** by removing
features:

- **Simple Mode** is the default a brand-new user lands in
  after trusting a project. Chat-first, single-column-leaning,
  more whitespace, fewer simultaneous panels. The provider
  strip is not exposed; Plume picks a sane provider or shows
  one friendly "set up a model" card. The file tree is hidden
  until the user asks for it. The status strip shows only
  model/memory telemetry (model name + memory pressure); trust,
  Close, and the mode toggle remain visible as persistent
  project controls. The mode-card grid (Chat / Propose diff /
  Scoped edit / Agent) does not appear unless the user opens an
  advanced disclosure.
- **Developer Mode** is the dense three-zone shell that ships
  today (D1.5 + D5 + D6 + everything since). All chips,
  panels, mode cards, the provider strip, the file inspector,
  the full status strip.

The principle that draws the boundary: **Simple shows the
single most useful thing per concern; Developer adds the
breakdown next to it.** Simple shows model name + memory;
Developer adds provider category, model details, swap, load
average. Simple shows a chat textarea + Send; Developer adds
the propose-diff segmented control, the context-preview row,
and the AGENTS.md badge above the same textarea. Simple hides
the file tree; Developer ships it open.

Simple and Developer share IPC. They differ only in what gets
rendered. A user can flip between them with no in-flight state
loss — a streaming chat continues whether or not the panels
around it are visible.

The toggle is per-project and persists across project opens
once persistence lands (`.plume/` on the project root, same
surface that holds approvals — see `docs/ARCHITECTURE.md`).
Until that lands, the mode lives in frontend state and defaults
to Simple on every project open. See `docs/UI_STYLE.md § Simple
Mode vs Developer Mode` for the visual rules and
`docs/IPC_ROADMAP.md § Session mode and policy` for the
graduation path.

#### Why this matters now

Today's trusted-project shell is honest about what Plume can
do, but it's loud. The hand-drawn cafe identity is part of why
this project exists — § 3 Visual Identity and `docs/UI_STYLE.md`
both make that load-bearing. The risk is that future slices
keep adding chips, panels, and badges to the dense shell
without anyone noticing that the calm-cafe default has been
quietly lost. Writing the Simple / Developer boundary now —
before the slice that implements it — gives every future UI
slice a place to ask "is this a Simple-Mode default or a
Developer-Mode reveal?" and forces the answer in review
instead of after merge.

## 8. Non-Goals For MVP

Plume should not try to be all of these at once:

- Full VS Code replacement.
- Full Cursor replacement.
- Full Codex clone.
- Full model training app.
- Full Hugging Face browser.
- Full package manager for every model format.
- Full cloud collaboration product.
- Full plugin marketplace.
- Full browser automation agent.
- Full remote development environment.
- A separate "beginner" product. Simple Mode and Developer Mode
  (see § 7.7) are two renders of the same app over the same IPC,
  not two SKUs. Simple Mode is not a feature-removed build, and
  Developer Mode is not a power-user-only build.
- Hands-on-desktop computer use. The agent does not click on,
  type into, or screenshot the user's macOS desktop in MVP. A
  scoped computer-use track is post-MVP — see § 13.5 — and even
  there, the first phase is a Plume-controlled in-app sandbox,
  not the host.

The first version should be a local coding editor with model-aware chat, scoped edits, diffs, and verification.

## 9. Technical Stack

Recommended stack:

- Desktop shell: **Tauri**.
- Backend: **Rust**.
- Frontend: **TypeScript**.
- UI framework: **React**.
- Editor: **CodeMirror 6**.
- Styling: plain CSS or CSS modules.
- Local storage: SQLite through Rust, or simple app data files at first.
- Model runtime control: Rust process manager calling provider CLIs or HTTP APIs.

Why Tauri:

- Lighter than Electron.
- Uses system WebView instead of shipping a full Chromium copy.
- Rust backend is a good fit for local filesystem, process control, sandbox rules, and app packaging.
- Still lets us build a polished UI in TypeScript/React.

Why not Electron first:

- Electron is fast to build with, but it bundles Chromium and tends to use more memory.
- This app is specifically about saving memory for local models.
- If the model already needs several GB, the shell should be lean.

Why CodeMirror:

- Lightweight compared with embedding a full VS Code experience.
- Good enough for editing, syntax highlighting, selections, search, and diff integration.
- Easier to style into the Plume visual identity.

Possible future alternative:

- Zed extension or Zed fork.

Zed is serious, native, Rust-based, and already editor-shaped. But it is a heavier starting point if the goal is a unique hand-drawn product. Tauri gives more control over the identity and app flow.

## 10. Runtime And Model Strategy

Plume needs a provider layer. The app should not care whether the model comes from MLX-LM, Ollama, LM Studio, llama.cpp, or another runtime.

The provider layer should expose one common interface:

- List available models.
- Check whether provider is installed.
- Start model server if needed.
- Stop model server if Plume owns it.
- Send chat/completion request.
- Stream tokens.
- Report model metadata.
- Report context window.
- Report estimated memory cost when possible.
- Report active provider health.

### 10.0 Runtime categories

Plume's relationship with a runtime sits on two axes.

**Process ownership.** Either Plume starts and supervises the runtime, or Plume connects to a daemon the user already has running.

**Integration depth.** Either Plume drives the model directly through the provider trait, or Plume embeds an external agent runtime and acts as the cockpit around it.

The first axis splits providers into two tiers:

- **Plume-managed runtimes.** Plume owns the process. MLX-LM and llama.cpp are the clearest fit. Ollama lands here when Plume itself starts the daemon.
- **Connected runtimes.** Plume connects to whatever the user is already running. Ollama (when the daemon is already up), LM Studio, and other OpenAI-compatible local servers live here.

The preferred Mac path is Plume-managed MLX. Ollama stays supported
because it is common and useful, but it is a fallback/compatibility path,
not the product center. Plume should eventually let the user import,
download, verify, and run MLX-format Gemma/Qwen-style weights without
needing a separate model manager daemon.

The second axis carves out a different track:

- **External agent engines.** Codex CLI, Claude Code, and OpenCode are agent runtimes, not LLM endpoints. When Plume embeds them, the engine owns the agent loop; Plume keeps the editor, the safety layer, the trust prompt, and the visible UI. See `docs/MODEL_PROVIDERS.md § External agent engines`.

These categories do not overlap with the model capability tiers in § 11 — capability is about what the model can do; category is about what Plume is responsible for.

### 10.1 Provider: MLX-LM

Primary Mac-first provider.

Why:

- MLX is optimized for Apple Silicon.
- Locally AI and Gemma Chat style apps show that MLX can feel much better than GGUF paths on Mac for certain models.
- MLX-LM can run Hugging Face MLX-format models like Gemma and Qwen variants.

Expected model examples:

- Gemma 4 E2B / E4B MLX.
- Qwen 2.5 / Qwen 3 / Qwen 3.6 MLX variants.
- Other MLX community conversions.

Open questions:

- Which MLX-LM server API shape is stable enough to rely on?
- How much custom config is needed per model family?
- How to handle tokenizer quirks, EOS tokens, tool-call formats, and chat templates?
- How should Plume safely reference weights already managed by another local app without duplicating large files?

### 10.2 Provider: Ollama

Useful because Codex and many tools already integrate with it.

Limits:

- On the user's current Mac setup, Ollama was using GGUF/Metal for the tested Gemma model, not active MLX.
- Ollama has MLX preview support, but model/runtime availability matters.
- Ollama can still load models with large memory use if context is high.

Plume should support Ollama because it is common, but it should not depend on Ollama for the best Mac experience.

If a future smoke test only works by saying "install Ollama first", that
is acceptable as a temporary compatibility test, not as the final local
model story.

### 10.3 Provider: LM Studio

Useful as a user-friendly local model manager and API server.

Plume can treat LM Studio as:

- A model download UI.
- A local server.
- A provider backend.

Plume should not assume LM Studio is the editor. LM Studio is more like the engine room; Plume is the coding workspace.

### 10.4 Provider: llama.cpp

Useful for cross-platform GGUF support.

Possible use:

- Linux.
- Windows.
- Older models.
- Users who prefer raw llama.cpp servers.

### 10.5 Provider: DFlash / Speculative Decoding

DFlash-style speculative decoding should be treated as an optimization layer, not magic.

Plain-English model:

- A small draft model guesses upcoming tokens.
- A larger model checks the guesses.
- If guesses are right, output moves faster.
- The big model still must fit in memory.

This can improve speed, but it does not shrink a 35B model into a 16 GB laptop by itself.

Plume should document this honestly in the UI.

### 10.6 External agent engines: Codex CLI, Claude Code, OpenCode

Open-sourced agent runtimes are a different category than LLM providers. They already own the read/plan/edit/test loop, prompt construction, and a model client. Wiring them through Plume's `Provider` trait would fight their grain.

The plan when this lands:

- Treat them as engines, not providers.
- Plume embeds the engine in a project session; the engine drives its own agent loop.
- Plume keeps cockpit responsibilities: editor, file tree, project trust prompt, path/command/patch safety, approval ledger, visible UI for humans and computer-use agents.
- Cloud-backed model use through these engines is opt-in and explicit, same as any other cloud call.

This is post-MVP and stays optional. Local provider mode is still the default. Naming the track now keeps the provider trait honest about its scope, and keeps Plume's positioning clear: when external agent runtimes are commodity, Plume is the cockpit, not just another LLM client.

## 11. Model Capability Tiers

Plume should classify models by what tasks they are likely to handle.

### Tiny / Fast

Examples:

- 1B-3B models.
- Small Gemma or Qwen variants.

Good for:

- Explaining snippets.
- Renaming variables.
- Writing small functions.
- Answering project questions with narrow context.

Bad for:

- Multi-file refactors.
- Complex debugging.
- Architecture changes.

### Small / Useful

Examples:

- 4B-8B coding or instruction models.

Good for:

- Single-file edits.
- Small bug fixes.
- Test suggestions.
- Simple diffs.

Needs:

- Strong prompting.
- Short context.
- User review.

### Medium / Capable

Examples:

- 14B-32B quantized models, if machine supports them.

Good for:

- Multi-file edits.
- Better planning.
- More reliable reasoning.
- Larger repo questions.

Risk:

- May not fit on 16 GB Mac.
- May need lower context or external runtime.

### Large / Workstation

Examples:

- 35B+ models.

Good for:

- Serious coding agent loops.

Risk:

- Not realistic for most 16 GB laptops.
- May need 32 GB+ unified memory or more.

Plume should not advertise these as "run on any laptop" unless proven by benchmarks.

## 12. Resource And Memory Design

Plume should make resource use visible.

Important resource signals:

- Provider running or stopped.
- Model loaded or unloaded.
- Model size on disk.
- Approximate memory needed.
- Context length.
- KV cache impact.
- Token generation speed.
- Current memory pressure.
- Whether the app or provider owns the model process.

For 16 GB Macs, default settings should be conservative:

- Prefer 4-bit quantized models.
- Prefer MLX models that are known to fit.
- Keep context lower by default.
- Warn before 32k, 64k, or 128k contexts.
- Avoid auto-loading huge models.
- Offer "safe mode" model presets.

Do not confuse context with intelligence. A huge context can blow memory even if the model weights fit.

## 13. Agent Workflow

Plume should use a staged coding loop.

### Stage 1: Chat

The model answers questions about code and explains snippets.

Allowed:

- Read selected file.
- Read visible context.
- Explain.
- Suggest.

Not allowed:

- Edit files without permission.
- Run commands without permission.

### Stage 2: Proposed Diff

The model proposes a patch, but the user applies it.

Allowed:

- Read specific files.
- Produce unified diff.
- Show explanation.

Not allowed:

- Auto-write.

### Stage 3: Scoped Edit

The model edits files after a clear plan.

Allowed:

- Modify approved files.
- Show diff.
- Run approved verification commands.

Needs:

- File allowlist.
- Command approval.
- Revert path.

### Stage 4: Agent Mode

The model loops through read, edit, test, fix.

Only for stronger models or explicit user approval.

Needs:

- Task budget.
- File scope.
- Command scope.
- Max iteration count.
- Clear stop conditions.
- Full diff review.

This maps to model strength. A tiny local model should default to Stage 1 or 2. A stronger Qwen coder model can try Stage 3. Large models can try Stage 4 if hardware allows.

## 13.5 Computer-Use Track (Post-MVP)

Stages 1-4 are about how much autonomy the model has WITHIN the
codebase. A separate, orthogonal axis is whether the model can
act *outside* the codebase — drive a browser, click through a
UI, capture a screenshot of an external app. That axis is the
**computer-use track**.

It is post-MVP and explicitly listed in § 8 Non-Goals. This
section reserves the shape so a future slice doesn't have to
reinvent the boundary.

The track exists on a DIFFERENT axis than agent operability. To
keep the two clear:

- **Agent operability** is Plume as a RECEIVING surface: external
  agents drive Plume's UI through ordinary OS accessibility,
  keyboard, and mouse. See `docs/AGENT_OPERABILITY.md`.
- **Computer-use track** is Plume as an EMITTING surface: the
  model running locally in Plume's chat path drives some target
  environment on the user's behalf. See `docs/IPC_ROADMAP.md §
  Computer use` for the verb shapes and `docs/SAFETY.md §
  Computer-use sandbox` for the safety contract.

The two never share IPC or approval state. A user who is fine
with external accessibility agents driving Plume is NOT
implicitly fine with Plume's model driving their host desktop;
the inverse is true too.

### Phase split

1. **Phase A — bundled webview sandbox.** Plume opens a webview
   it controls inside its own window. The model drives that
   webview — clicks, types, scrolls, captures, optionally reads
   the DOM as an accessibility tree. The target is fully
   Plume's territory: no host accessibility, no host screen
   capture, no host input synthesis. CSP is strict, network
   defaults to offline, disk access is blocked.
2. **Phase B — host desktop.** Plume drives the user's actual
   macOS desktop via accessibility APIs + `CGEvent` input
   synthesis + `CGWindowList` screen capture. **Off by default,
   per-session opt-in, per-target allowlist.** Enabling Phase B
   requires (1) project trust, (2) a foreground approval dialog
   naming the target, and (3) macOS-level accessibility +
   screen-recording permissions. Granting it for one session
   does NOT grant it for the next; there is no persistent
   approval ledger for computer-use sessions.

### Safety contract (forward-looking)

- Every session start shows a foreground approval dialog with no
  "remember this" toggle. The user re-reads every session. This
  is Plume's own per-session gate — it sits on top of, not
  instead of, the macOS-level Accessibility + Screen Recording
  permissions, which are app-persistent grants managed in
  System Settings → Privacy & Security.
- Every action emits a visible trace step. The trace area
  carries a Pause and a Stop button always.
- A target allowlist is mandatory; wildcards are not accepted
  entries. "Whole desktop" mode does not exist.
- `computer.capture` returns image bytes that the existing
  text-regex prompt-read redactor CANNOT rewrite (you cannot
  un-paint a secret-shaped substring in a PNG). Image safety
  rests on scaling/cropping and on the `targetAllowlist` — the
  user named the target, so a capture aimed there is the
  approved outcome, not a leak. Text Plume extracts from a
  capture (OCR, accessibility tree, DOM strings) DOES pass
  through the existing redactor before the model sees it. See
  `docs/SAFETY.md § Redaction before model sees frames` for the
  contract.
- There is no codepath from a Phase A approval to Phase B
  execution.

### Reference implementations

The track might integrate the upstream `trycua` / `cua-driver`
project (https://github.com/trycua) for the Phase B backend, or
implement the platform-API calls directly. Today: neither is
wired, no dependency added, no install required. The doc shape
leaves room for either choice — the slice that lands the track
will revisit the trade-off.

### Why this matters now

Plume's identity is "a calm local coding cafe." Hands-on-desktop
computer use is in a different posture entirely — it's a
power-tool that can quietly do a lot to a user's machine. The
risk is that the track ships ad-hoc, without the boundary
documents written, and the user discovers Plume has been
clicking through their browser tabs because a model emitted a
plausible-looking command. Writing the boundary now — before the
slice — makes it harder to slip past.

## 14. Safety Model

Plume should protect the user's files without making the app annoying.

Core rules:

- Never edit outside the opened project unless explicitly approved.
- Never run destructive shell commands without explicit approval.
- Show diffs before committing changes.
- Keep a session log of file changes.
- Make it easy to revert Plume-created edits.
- Treat model output as suggestions, not truth.
- Prefer project-native verification commands.

Potential safety features:

- File write allowlist.
- Command allowlist.
- Approval prompts for shell commands.
- Read-only mode.
- Git checkpoint before agent mode.
- "Undo last Plume edit" command.
- "Explain before apply" toggle.

## 15. Project Awareness

When opening a repo, Plume should inspect project truth before acting.

Read order:

1. `AGENTS.md`
2. `README.md`
3. package or build config files
4. existing verification scripts
5. git status

If both `AGENTS.md` and `CLAUDE.md` exist, Plume should prefer `AGENTS.md` and flag the duplicate as something to consolidate.

Plume should detect:

- Language and framework.
- Package manager.
- Test command.
- Lint command.
- Build command.
- Git branch and dirty files.
- Important project instructions.

The user experience should match good agent discipline:

- Read project truth first.
- Make scoped plan.
- Edit.
- Verify.
- Explain.

## 16. UI Layout Details

### 16.1 Left File Pane

Purpose:

- Browse project files.
- Open files.
- See changed files.
- Show model-read files.

Visual:

- Scribbled shelf-like outline.
- File rows should stay clean and compact.
- Modified files get simple hand-drawn dot or mark.

Features:

- Folder tree.
- Search file.
- Git changed filter.
- Recently opened.
- Files included in AI context.

### 16.2 Center Editor

Purpose:

- Main coding surface.

Features:

- CodeMirror editor.
- Syntax highlighting.
- Tabs.
- Find in file.
- Selection-to-chat.
- Inline diff preview.
- Read-only diff mode.

Visual:

- Code area mostly plain.
- Hand-drawn frame around editor.
- No heavy decorative texture behind text.

### 16.3 Right AI Pane

Purpose:

- Chat and agent control.

Sections:

- Prompt input.
- Context chips.
- Model selector.
- Mode selector: chat, propose diff, scoped edit, agent.
- Plan output.
- Tool activity.
- Apply/reject controls.

Visual:

- Like a notebook page or cafe receipt.
- Model messages can have paper panels.
- Tool events can be compact.

### 16.4 Bottom Terminal / Verification Pane

Purpose:

- Show commands, tests, logs, provider output.

Features:

- Run project verifier.
- Show command history.
- Expand/collapse logs.
- Show pass/fail.

Visual:

- Keep it mostly plain.
- Drawn border only.

### 16.5 Status Strip

Purpose:

- Always show the important truth.

Status items:

- Provider: MLX-LM / Ollama / LM Studio / llama.cpp.
- Model name.
- Context length.
- Memory pressure.
- Git branch.
- Dirty file count.
- Current mode.
- Offline/online state.

The memory indicator should be clear:

- Green: comfortable.
- Amber: watch it.
- Red: likely to hurt performance.

## 17. Model Picker

The model picker should not just list names. It should help users choose.

Fields:

- Name.
- Provider.
- Family.
- Parameters.
- Quantization.
- Context.
- Disk size.
- Estimated memory.
- Hardware fit.
- Coding score, if known.
- Best use.

Example display:

```text
Gemma 4 E4B MLX
Provider: MLX-LM
Fit: Good on 16 GB Mac
Best for: small edits, explanations, fast local chat
Default mode: propose diff
```

```text
Qwen Coder 14B Q4
Provider: MLX-LM or llama.cpp
Fit: Maybe on 16 GB Mac with low context
Best for: stronger code edits
Default mode: scoped edit
```

```text
Qwen 35B DFlash
Provider: MLX-LM + DFlash
Fit: Not for 16 GB unless proven
Best for: workstation coding
Default mode: manual approval only
```

## 18. Prompting Strategy

Plume should use different prompts for different model sizes.

Small local models:

- Shorter system prompt.
- Concrete instructions.
- Smaller context.
- One task at a time.
- Ask for patch only when needed.

Large models:

- More complete agent prompt.
- More project context.
- Multi-step planning.
- Tool use loops.

Do not feed a small Gemma model a giant Codex-style employee handbook unless needed. That wastes context and can make it worse.

Prompt sections:

- Role: local coding assistant.
- Project rules: from `AGENTS.md`.
- Task.
- Allowed files.
- Relevant context.
- Output format.
- Safety limits.

## 19. Context Management

Context is one of the hardest parts.

Plume should support:

- Selected text context.
- Open file context.
- Specific file attachments.
- Repo map.
- Search results.
- Git diff context.
- Test output context.

It should avoid:

- Dumping the whole repo.
- Dumping huge files without reason.
- Including generated files by default.
- Including secrets.

Potential context pipeline:

1. User asks task.
2. Plume detects likely files.
3. Plume asks model for missing context questions if needed.
4. Plume uses search to gather snippets.
5. Plume builds compact context packet.
6. Model responds with plan or diff.

For small models, context should be extra curated.

## 20. File Editing Design

Plume should represent edits as patches.

Why:

- Easy to review.
- Easy to apply.
- Easy to reject.
- Easy to log.
- Easy to revert.

Editing flow:

1. Model proposes patch.
2. Rust backend validates patch paths.
3. App displays diff.
4. User applies.
5. Backend writes files.
6. Git status refreshes.

Later, Plume can support direct editor edits, but patches are safer for agent output.

## 21. Verification Design

Plume should discover and run project-native verification.

Detection examples:

- `npm run verify`
- `npm test`
- `pnpm test`
- `cargo test`
- `pytest`
- `./scripts/verify.sh`

For the user's workflow, verification matters before git work. Plume should treat hooks and CI as backup, not replacement.

Verification UI:

- Shows command.
- Shows whether user approved it.
- Streams output.
- Summarizes failure.
- Offers model a focused failure context.

## 22. Git Design

MVP Git features:

- Show branch.
- Show dirty file count.
- Show changed files.
- Show diff.
- Create checkpoint before risky agent run.

Later Git features:

- Stage selected files.
- Commit with generated message.
- Review recent changes.
- Handoff review mode.

Plume should never push by default.

## 23. App Data

Local app data may include:

- Provider settings.
- Model registry cache.
- Recent projects.
- Session transcripts.
- Tool logs.
- User preferences.
- UI layout.

Privacy rule:

- Store locally.
- Make logs inspectable.
- Let user clear project/session history.
- Do not upload by default.

Possible storage:

- SQLite for structured data.
- JSON/TOML for config.
- Plain files for logs.

## 24. Suggested Repository Structure

Future repo shape:

```text
plume/
  AGENTS.md
  README.md
  package.json
  src/
    app/
      main.tsx
      App.tsx
      styles/
    features/
      editor/
      file-tree/
      ai-panel/
      terminal/
      model-picker/
      diffs/
      settings/
    lib/
      api/
      context/
      prompts/
      models/
  src-tauri/
    Cargo.toml
    src/
      main.rs
      commands/
      providers/
      project/
      safety/
      git/
      process/
  docs/
    PLUME_PROJECT_SPEC.md
    ARCHITECTURE.md
    MODEL_PROVIDERS.md
    UI_STYLE.md
  scripts/
    verify.sh
  tests/
```

Boundary idea:

- Frontend owns UI state and user interaction.
- Rust backend owns filesystem, processes, provider calls, safety checks, and git.
- Provider modules hide runtime differences.
- Prompt modules hide model-specific prompting.

## 25. Rust Backend Modules

Expected backend modules:

- `project`: open folders, detect project type, read instructions.
- `fs`: safe reads/writes inside project root.
- `git`: status, diff, checkpoint, branch info.
- `providers`: common model provider trait.
- `providers/mlx_lm`: MLX-LM adapter.
- `providers/ollama`: Ollama adapter.
- `providers/lmstudio`: LM Studio adapter.
- `providers/llamacpp`: llama.cpp adapter.
- `process`: start/stop provider processes.
- `safety`: approvals, path validation, command validation.
- `commands`: terminal command runner.
- `patch`: validate and apply diffs.
- `settings`: app config.

Important backend principle:

The frontend should not directly run shell commands or write arbitrary files. It asks the backend, and the backend enforces rules.

## 26. Frontend Modules

Expected frontend modules:

- `EditorPane`
- `FileTree`
- `AIPanel`
- `ModelPicker`
- `DiffViewer`
- `TerminalPane`
- `StatusStrip`
- `Settings`
- `SessionTimeline`
- `ApprovalModal`

Styling modules:

- `ink.css`: hand-drawn borders, paper panels, line weights.
- `layout.css`: panes and resizing.
- `tokens.css`: colors, spacing, typography.

Avoid a huge global CSS mess. Keep visual primitives small.

## 27. UI Components

Plume should have its own small design vocabulary.

Components:

- InkButton
- InkIconButton
- InkPanel
- InkTab
- InkBadge
- InkInput
- InkSelect
- InkToggle
- InkSlider
- InkDivider
- InkTooltip
- InkModal

Each component should support:

- keyboard navigation
- focus states
- disabled states
- compact layout
- predictable sizing

Hand-drawn does not mean sloppy. It means deliberately imperfect borders around a solid UI.

## 28. Accessibility

Plume must stay readable.

Requirements:

- High contrast text.
- Keyboard-accessible controls.
- Clear focus rings.
- Stable accessible names and roles for every important control.
- Visible status/error/progress text that external computer-use agents can
  inspect without reading logs.
- No tiny decorative-only labels.
- No animation required to understand state.
- Respect reduced motion.
- Terminal and code fonts large enough by default.

The scribbled style should not become visual noise.

Agent-operability is part of accessibility. Plume should be controllable
through the same visible UI a human uses: mouse, keyboard, text entry, and
accessibility tree. Do not build hidden automation-only paths for normal
product workflows.

## 29. Performance Requirements

MVP performance goals:

- App shell should open quickly.
- Idle memory should stay low.
- UI should remain responsive while model runs.
- Model output should stream.
- File tree should handle normal student/indie project sizes.
- Terminal output should not freeze UI.

Important:

- The app shell must not steal memory the model needs.
- Avoid Electron unless there is a strong reason.
- Avoid huge frontend libraries.
- Avoid heavy animation.
- Avoid indexing entire repos in memory.

## 30. Packaging

Target platforms:

- macOS first.
- Apple Silicon first.
- Intel Mac later if realistic.
- Linux later.
- Windows later.

Distribution for MVP:

- GitHub repo.
- Dev build instructions.
- Tauri dev app.

Later:

- Signed macOS app.
- Auto-update.
- Model setup wizard.

## 31. MVP Feature Set

MVP should include:

- Open local project folder.
- Read `AGENTS.md` and README.
- File tree.
- Code editor.
- Local model provider settings.
- MLX-LM provider first, if feasible.
- Ollama provider fallback.
- Chat about selected files.
- Propose patch.
- Apply patch after approval.
- Show diff.
- Run verification command after approval.
- Show model and memory status.

MVP should not include:

- Full plugin system.
- Cloud sync.
- Team collaboration.
- Automatic indexing of huge repos.
- Complex notebook support.
- Full VS Code extension compatibility.

## 32. Milestone Plan

### Milestone 0: Research Spike

Goals:

- Test Tauri + CodeMirror shell.
- Test MLX-LM server from app.
- Test one Gemma model.
- Test one Qwen model.
- Measure memory.

Exit criteria:

- Can open app.
- Can type code.
- Can stream model response.
- Can report basic memory info.

### Milestone 1: Editor Shell

Goals:

- Project picker.
- File tree.
- Editor pane.
- Status strip.
- Basic Plume visual style.

Exit criteria:

- Open folder and edit files manually.
- UI feels like Plume, not generic template.

### Milestone 2: Model Provider Layer

Goals:

- Provider trait.
- MLX-LM adapter.
- Ollama adapter.
- LM Studio adapter if easy.
- Model picker.

Exit criteria:

- Switch providers.
- Stream chat from at least two providers.

### Milestone 3: Context And Chat

Goals:

- Attach selected file.
- Attach selected text.
- Read project instructions.
- Ask questions about code.

Exit criteria:

- Model can explain file with explicit context.

### Milestone 4: Diff Proposals

Goals:

- Model outputs patch.
- App validates patch.
- Diff viewer displays patch.
- User applies/rejects.

Exit criteria:

- Safe single-file model edits.

### Milestone 5: Verification Loop

Goals:

- Detect test/verify commands.
- Ask approval.
- Run command.
- Stream output.
- Feed failures back to model.

Exit criteria:

- Model can fix a simple failing test with user approval.

### Milestone 6: Agent Mode

Goals:

- Scoped multi-step loop.
- Max iterations.
- File allowlist.
- Command allowlist.
- Session log.

Exit criteria:

- Handles a small real bug without wrecking files.

## 33. Testing Strategy

Test levels:

- Rust unit tests for path safety, provider parsing, patch validation.
- Frontend component tests for important UI states.
- Integration tests for provider adapters with mocked servers.
- Manual app tests for real local model runs.
- Agent smoke tests against a bundled local app window.
- Memory tests on 16 GB Mac.

Critical test cases:

- Reject patch outside project root.
- Reject destructive commands without approval.
- Load project with `AGENTS.md`.
- Prefer `AGENTS.md` over `CLAUDE.md`.
- Apply valid patch.
- Reject invalid patch.
- Stop provider process owned by Plume.
- Do not stop provider process not owned by Plume.
- Handle model server unavailable.
- Handle model stream interruption.
- Handle verification failure.
- Agent can open/trust a project, browse files, open CodeMirror, and trigger
  blocked-file behavior through the visible UI.

## 34. Security And Privacy

Threats:

- Model suggests destructive command.
- Model writes outside project.
- Prompt injection from files.
- Secret leakage into logs.
- Accidental cloud call.
- Malicious repo instructions.

Defenses:

- Path sandbox.
- Command approval.
- Default local-only mode.
- Secret pattern redaction in prompts/logs where practical.
- Clear provider labels.
- Trust prompt for new projects.
- No auto-run from repo instructions.

Important:

Plume should never blindly obey instructions found inside a repository. Project docs guide behavior, but user approval and app safety rules win.

## 35. Documentation To Maintain

Docs needed later:

- `README.md`: what Plume is and how to run it.
- `docs/ARCHITECTURE.md`: app architecture.
- `docs/MODEL_PROVIDERS.md`: provider setup and supported models.
- `docs/UI_STYLE.md`: visual system.
- `docs/AGENT_OPERABILITY.md`: visible UI operability contract.
- `docs/SAFETY.md`: file and command safety rules.
- `docs/DEVELOPMENT.md`: local dev setup.

This current spec should stay as the long product brief.

## 36. Open Questions

Product questions:

- Should Plume be a standalone editor or a companion app first?
- Should the first MVP edit real files or only produce diffs?
- Should chat history be saved by default?
- Should Plume have a "student mode" that explains changes more?

Model questions:

- Which MLX-LM model gives the best coding value on 16 GB?
- Is Gemma 4 E4B enough for useful scoped edits?
- Which Qwen Coder model fits comfortably?
- Can DFlash improve speed enough to matter on Mac?
- How stable are MLX-LM server APIs?

Design questions:

- How much sketch texture is charming versus distracting?
- Should the default be light-only, or include dark ink mode?
- How cafe-like should it be before it feels gimmicky?

Technical questions:

- How much filesystem indexing is needed?
- Should embeddings be local for repo search?
- Should provider processes be managed by Plume or user-managed?
- Should app data be SQLite from day one?

## 37. First Implementation Recommendation

The best first move is not to build the whole agent.

Build this first:

1. Tauri app opens.
2. UI has Plume visual style.
3. User opens a folder.
4. CodeMirror displays files.
5. App can connect to one local provider.
6. User can send selected code to model.
7. Response streams back.

That proves the core loop and the vibe.

Then add patch proposals.

Then add verification.

Then add agent mode.

Do not start with full autonomy. Start with a beautiful local coding desk that can talk to a local model. Let the agent grow after the foundation feels good.

## 38. Final Product Sentence

Plume is a hand-drawn local AI coding editor: a quiet black-and-white coding cafe that runs open models through lightweight native tooling, respects laptop memory, and gives students and indie hackers a private Codex-style workflow without pretending small local models are magic.
