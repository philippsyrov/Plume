# Agent Operability

Plume must be usable by visual and keyboard-driving agents through the
same UI a human uses. This is a product requirement, not a testing hack.

The goal is not to add a hidden "agent god mode." The goal is to make the
visible desktop app complete, legible, and controllable enough that a
human, a screen reader user, and an external computer-use agent can all
drive the same workflows.

## Core Rule

If a human can do an important action in Plume, an agent should be able to
do it through the visible UI with normal input:

- mouse clicks
- keyboard navigation
- text entry
- accessibility labels and roles
- visible status, errors, progress, and approval prompts

The backend may expose typed IPC for the frontend, but product workflows
must not depend on a private automation-only path that bypasses the UI.

## UI Contract

Every interactive surface should have:

- A stable accessible name that describes the action or state.
- A real role: button, textbox, list, list item, tab, dialog, status, alert.
- Keyboard access with visible focus.
- Disabled/loading states that are visible and reflected to accessibility.
- Error text that is visible on screen, not only logged.
- Progress/cancel controls for long-running work.

This applies to file browsing, editor tabs, model picker controls, approval
dialogs, diff review, terminal output, command runners, provider status,
and future agent-loop controls.

## Workspace shell zones

Once a project is trusted, the visible UI is a three-zone shell. Each
zone is a `section` with a stable accessible name an agent can target:

- "Project files" — left zone. File navigator with breadcrumb and
  listing. Driving this is how an agent picks a file to inspect.
- "Agent workspace" — center zone. Carries the "Selected model"
  banner (D6), the read-only "Chat" panel (D7 + D7.1 streaming +
  D8 attach + D10 selection range + D11 AGENTS.md auto-context),
  and the mode-card grid that
  names what's still planned. The Chat panel has a visible
  `read-only` badge and a subtitle stating it forwards your text
  to the model and that an optional file attachment goes through
  a backend secret redactor; the prompt textarea has a `Message
  to send` accessible label. When the trusted project has a root
  `AGENTS.md`, a badge sits next to the read-only badge and
  flips between three states based on the backend's per-send
  confirmation:
  - `¶ AGENTS.md available` before any send has resolved
    (forward-looking promise from `ProjectMeta.hasAgentsMd`);
  - `¶ AGENTS.md included` after the backend confirmed the last
    send folded the file in as system context;
  - `¶ AGENTS.md skipped` (warn-colored) after the backend
    reported the last send did NOT include the file — the user
    can investigate whether it's oversize, binary, or otherwise
    unreadable. The badge never claims "included" from project
    metadata alone. While a reply is streaming the Send
  button is replaced by a Stop button (accessible label `Stop
  streaming reply`) and the in-progress assistant entry shows a
  blinking cursor glyph plus a `streaming…` meta line. The
  "Read-only file context" row above the textarea exposes the
  attach control. Its label depends on whether the user has a
  non-empty text selection in the file inspector:
  - `Attach current file` (or `Replace with current file` when a
    chip already exists) when the editor's cursor is a point /
    no selection — D8 whole-file behavior;
  - `Attach selection` (or `Replace with selection` when a chip
    already exists) when the editor has a non-empty selection —
    D10 line-range behavior. The accompanying hint reads
    `Inspector has lines X–Y of <path> selected.` so an agent
    knows what attaching will send.
  A chip then shows the project-relative path, optionally with a
  trailing `:start–end` (or `:N` for single-line picks), and the
  chip's `×` button has the accessible name `Remove attached
  file <path>` or `Remove attached selection <path>:X–Y`. An
  agent driving the panel:
  - waits for `Selected model` to show a picked model before
    submitting,
  - (optional) opens a file in "File inspector" and either keeps
    the cursor as a point (whole file) or selects a range
    (lines X–Y); the inspector's editor surfaces line numbers
    through its gutter and the attach button's `title` describes
    exactly what will be attached. The button is disabled with a
    descriptive `title` when the inspector selection is binary,
    oversize, blocked, or loading,
  - types into the textarea with role `textbox`,
  - presses Send (or Enter); Shift+Enter inserts a newline,
  - reads new entries from the transcript list, which has
    `aria-live="polite"` and `aria-relevant="additions text"` so
    screen readers and computer-use agents are notified both when
    a turn appears and as each delta is appended,
  - sees an attachment chip rendered inline on the user turn that
    carried it (accessible name `Attached: <path>` or
    `Attached: <path>:X–Y`),
  - reads the D12 "Will ride along" preview area between the
    attach bar and the textarea — a small row that lists each
    piece of context the next send would carry. AGENTS.md
    surfaces as `¶ AGENTS.md · <bytes>` (plus `· N redactions`
    when the redactor matched anything); a ready attachment as
    `¶ <path>[:X–Y] · <bytes>`; an attachment that the backend
    would refuse as `⚠ <path> · would be blocked · <reason>`,
    warn-coloured, with the typed reason and the full IpcError
    text in the `title` tooltip. The preview is fed by the
    read-only `chat.context` IPC and refreshes whenever the chip
    or AGENTS.md state changes — so an agent driving the panel
    can read the live, backend-confirmed answer to "what will
    actually get sent?" before it presses Send,
  - clicks Stop to cancel the in-flight stream; the partial reply
    stays visible with a `stopped by you` meta line.
- "File inspector" — right zone. Header strip plus the read-only
  CodeMirror view (or a blocked / binary / empty placeholder). The
  header always shows the path of the current selection so an agent
  can confirm what it is reading.

The "Local model providers" panel sits under the navigator in the
left zone. Each model row carries a Select button — disabled when
the provider's reachability is not `available`, so an agent cannot
fake a selection against an offline runtime. Selecting a model
updates the "Selected model" banner in the center zone; the picked
row also gains a `✓ selected` badge. The Close button stays on the
project status strip above the shell.

When chat and model loading land, the same accessible names persist;
new affordances become real controls under existing labels rather
than new hidden surfaces.

## Safety Gates

Approval and trust gates must stay visible. An agent can click the same
button the user would click, but it must not get a private bypass.

Examples:

- Trusting a project happens through the trust prompt.
- Running a shell command happens through the command approval UI.
- Applying a patch happens through the diff approval UI.
- Revoking permissions happens through the visible permissions UI.

If an action is dangerous for a human, it is dangerous for an agent. The
UI must make that danger legible before the action is allowed.

## Command Palette

A command palette can help both humans and agents. It should expose the
same actions as the visible UI, not extra hidden powers.

Good palette actions:

- Open project
- Trust current project
- Focus file tree
- Open selected file
- Run approved verifier
- Cancel current run
- Show permissions
- Revoke approval

Bad palette actions:

- Trust arbitrary path without opening it.
- Run arbitrary shell without approval.
- Feed raw display reads into prompts.
- Apply patches without the diff approval path.

## Smoke Harness

Dev-mode Tauri binaries are not discoverable by macOS LaunchServices
because `tauri dev` runs a raw binary at `src-tauri/target/debug/plume`
instead of producing an installed `.app` bundle. Accessibility APIs,
the computer-use MCP, and screen-sharing automation all key on the
LaunchServices registry — they cannot allowlist or address a raw
binary. That is fine for manual development but not enough for Plume's
agent-operability goal.

`scripts/smoke-app.sh` is the launch path. It is real, not a skeleton:

```bash
./scripts/smoke-app.sh
```

Behavior:

- Builds a debug-profile `Plume.app` bundle via
  `npm run tauri -- build --debug --bundles app` (routed through
  `./scripts/dev-env.sh`).
- The bundle lands at
  `src-tauri/target/debug/bundle/macos/Plume.app` — a real macOS
  application directory with the bundle id `dev.plume.app`.
- Launches via `open`, which registers the `.app` with macOS
  LaunchServices so accessibility tools and computer-use agents can
  allowlist `Plume` and target its window.
- Prints the bundle path and the two ways to read logs (Console.app
  filtered by subsystem, or running the inner binary directly with
  `PLUME_LOG=info` for stdout in-shell).
- Refuses to run on non-macOS until that path is implemented.
- Refuses to build with missing icons and prints the regeneration
  recipe (`sips` + `npx tauri icon`).

Constraints kept:

- No network/model downloads. The script exports `CARGO_NET_OFFLINE=true`
  before building; if the project-local Cargo cache is missing a crate
  the build fails non-zero and prints the one-time prefetch command
  (`./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo fetch'`).
- No new dependencies.
- The same trust prompt and approval gates apply — the `.app` is the
  same code as `tauri dev`, just packaged so macOS can see it.
- A previously-launched smoke instance is quit before `open` so the
  freshly built bundle is what runs (otherwise macOS would re-activate
  the stale instance and the user would silently test old UI).

The smoke path is what lets computer-use agents test the actual
desktop window: open a project, trust it, browse files, open
CodeMirror, trigger blocked-file behavior, close the project.
The repeatable checklist lives in `docs/SMOKE_TESTING.md`.

## Testing Standard

Future UI slices should include an agent-operability check in their review:

- Can the main action be reached by keyboard?
- Does the target control have a useful accessible name?
- Does the state change produce visible text?
- Can an external agent recover if the action fails?
- Are approval prompts visible and actionable?
- Does the workflow still use the same safety gate as human use?

This is not separate from accessibility. Agent-operable UI is accessible UI
with stricter workflow expectations.
