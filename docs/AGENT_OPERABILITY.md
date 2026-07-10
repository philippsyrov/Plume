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
  banner (D6; D89 added inline Start / Stop / running controls for a
  selected Plume-managed MLX model, so the model you chat with can be
  brought online from the chat zone) and the read-only "Chat" panel
  (D7 + D7.1 streaming + D8 attach + D10 selection range + D11
  AGENTS.md auto-context). (D87 removed the descriptive mode-card grid;
  the response-mode toggle lives in the chat header and the agent-mode
  + gates in the left-column "Agent settings" card.) The Chat panel has
  a visible
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
  - reads the D12 "Context preview:" row between the attach bar
    and the textarea — a small area that lists each piece of
    context the next send would carry, plus any attachment the
    backend would REFUSE on that send (the label is intentionally
    neutral because a blocked attachment, which does not ride
    along, still belongs in the same section). AGENTS.md surfaces
    as `¶ AGENTS.md · <bytes>` (plus `· N redactions` when the
    redactor matched anything); a ready attachment as
    `¶ <path>[:X–Y] · <bytes>`; an attachment that the backend
    would refuse as `! <path> · would be blocked · <reason>`,
    warn-coloured, with the typed reason and the full IpcError
    text in the `title` tooltip. The preview is fed by the
    read-only `chat.context` IPC and refreshes whenever the chip
    or AGENTS.md state changes — so an agent driving the panel
    can read the live, backend-confirmed answer to "what will
    actually get sent?" before it presses Send,
  - clicks Stop to cancel the in-flight stream; the partial reply
    stays visible with a `stopped by you` meta line,
  - sees a D14 `Recheck` button (accessible label `Recheck <Provider> reachability`) next
    to the chat status when the selected model's provider is not
    reachable. While unreachable: the placeholder reads
    `Type your message — start <Provider> and click Recheck to send.`,
    the textarea stays enabled (so the user can compose), but Send
    is disabled until reachability resolves. The status row has
    `aria-live="polite"` so reachability flips are announced,
  - on a completed assistant turn, sees a subtle `Copy` button
    (accessible label `Copy assistant reply text to clipboard`)
    at the top-right of the entry. Copying writes the full reply
    text via `navigator.clipboard.writeText`; the button flips to
    `Copied!` for ~2 s, then back. Streaming and cancelled turns
    deliberately do not expose the Copy button — a partial reply
    could mislead the user about what they captured,
  - on a rejected send (e.g. Ollama not running, validation
    rejection from the backend), the attachment chip is RESTORED
    after the error row appears — D14: rejected sends are not
    consumed, so the user can fix the underlying issue (start the
    daemon, swap the model, restore a missing file) and retry
    without re-attaching,
  - sees a D15 segmented mode toggle in the chat header next to
    the Clear button: `Chat | Propose diff` (role `radiogroup`,
    aria-label `Response mode for next send`, each option role
    `radio` with `aria-checked` reflecting selection). Active
    option fills with ink-black; inactive option stays on paper.
    Disabled while a stream is in flight (flipping mid-stream
    would be confusing — the in-flight turn keeps the mode it
    was started with). Switching to `Propose diff` adds a
    `¶ propose diff` badge on the next user turn so the
    transcript history records why a reply is a diff,
  - on a `Propose diff` assistant reply, sees a rendered diff
    panel below the role label instead of plain prose. Per-line
    coloring: additions paired with `--good`, deletions with
    `--bad`, hunk headers in pencil, file headers bold. Aria
    labels announce `Added: <line>` / `Removed: <line>` for the
    diff lines that change content. Between the rendered diff
    body and the actions row, the panel renders a D16
    **validation pill** (`role="status"`, `aria-live="polite"`):
    `validating diff…` while the read-only `patch.validate` IPC
    is in flight, then `valid diff · N file(s) · M hunk(s)` (in
    `--good`) on success or `invalid diff: <headline reason>` (in
    `--bad`) on failure — the full error list is in the `title`
    tooltip so a screen reader user can read all errors via the
    accessible name. When the IPC itself fails (no trusted
    project, version mismatch, internal error) the pill shows
    `validation unavailable: <message>` in pencil so the diff
    renderer never disappears just because validation couldn't
    complete. Below the pill the panel exposes an **Apply**
    button that is **always disabled** today, regardless of
    validation outcome (accessible label flips between
    `Apply this diff (disabled — preview only)` and
    `Apply this diff (disabled — validation passed but apply is
    future)`; the title names the boundary either way) plus a
    `preview only — no writes` italic note. The Copy button on
    the assistant turn (D14) writes the FULL reply text —
    including the fence markers — so the user can paste it into
    `git apply -` or similar. When the model returns prose
    instead of a fenced diff in propose-diff mode, the panel
    shows a warn-coloured `No diff fence detected — model
    returned prose. Try again or rephrase the request.` hint
    instead of the diff renderer.
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

### Chat sessions in the sidebar (D63B)

The unified sidebar's chat rows are persisted sessions, and every
control is a labelled DOM element a visual agent can target:

- The sidebar is `aside` "Project navigation". `New chat` (top nav)
  creates a local session; the project row's `+` (`New project chat`)
  creates a project session. Local rows render under **Chats**,
  project rows under the project — the two lists never mix.
- Each row's `…` button is `Chat actions for <title>`
  (`aria-haspopup="menu"`); the popover exposes `Rename`, `Archive`,
  and `Delete` menu items. Rename and delete open Plume-styled
  dialogs (`role="dialog"`, labelled headings) — never
  `window.prompt` / `window.confirm`.
- Blocked actions are visible, not silent: switching chats while a
  reply streams surfaces a `role="status"` notice above the chat
  surface; a failed transcript save surfaces a `role="alert"` banner
  ("Chat history could not be saved…"). An agent should read those
  regions after acting instead of assuming the click landed.

## Mode toggle

The trusted-project shell renders in one of two modes, **Simple**
(default) or **Developer**. The product axis is described in
`docs/PLUME_PROJECT_SPEC.md § 7.7` and the visual rules in
`docs/UI_STYLE.md § Simple Mode vs Developer Mode`. This section
pins the accessibility contract.

The toggle lives on the right end of the project status strip,
inside the existing strip element, and is rendered in both
modes. It is a single visible control (role `switch`) with:

- An accessible name of `UI mode` and a current-value
  description in the accessible description — `Simple` or
  `Developer`. The switch's `aria-checked` is `true` when the
  mode is `Developer` and `false` when it is `Simple`, so an
  agent reading the accessibility tree can resolve the current
  mode without inferring from rendered chrome.
- Visible label text next to the switch — `Simple` and
  `Developer` — never reduced to an icon-only control. The
  hand-drawn cafe identity does not justify hiding what mode
  the user is in.
- Full keyboard accessibility — focusable in the tab order,
  toggled with Space, with a visible ink-colored focus ring.
  Activating the toggle from a screen reader uses the standard
  `switch` interaction.
- A live region (`aria-live="polite"`) on the strip so the
  switch's announced state change reaches inbound operability
  agents and screen readers without polling.

The status strip also exposes which mode is active as text on
the strip itself (the visible label next to the switch). An
operability agent observing Plume can therefore identify the
current mode three ways: the visible `Simple` / `Developer`
label, the `switch` control's `aria-checked` value, and (when
read after a flip) the live-region announcement. The mode is
never carried only in a visual difference between renders.

Flipping the toggle re-renders the shell. It does NOT cancel
in-flight chat, abort the streaming reply, or re-resolve the
selected model — Simple and Developer share the same chat IPC
(`chat.send`, `chat/token`, `chat/done`, `chat.cancel`,
`chat.context`, `patch.validate`), so the only thing changing
is what's painted. An agent driving Plume can rely on this:
flipping the mode is safe to do mid-conversation.

When a control is hidden by Simple Mode (the provider strip,
the file tree, the file inspector, the mode-card grid, the
propose-diff segmented control, the AGENTS.md badge, the
context-preview row), it is removed from the accessibility tree
entirely — not just visually hidden. An agent that needs those
controls must flip the mode to Developer first. This keeps the
accessibility tree honest about what is currently reachable; it
does not create a hidden bypass.

## Plume as a computer-use HOST (post-MVP roadmap)

This document is about Plume as a **RECEIVING surface** — external
agents (Anthropic computer-use, Cursor, OS accessibility tooling,
screen readers) drive Plume through ordinary OS accessibility,
keyboard, and mouse paths. That's the core agent-operability
contract above, and it ships today.

A separate post-MVP track makes Plume an **EMITTING surface**:
the model — running locally, through Plume's chat path — gets a
typed `computer.*` tool family it can call to drive a target
environment on the user's behalf (clicks, types, scrolls,
captures screenshots, optionally reads a structured AX tree).
See `docs/IPC_ROADMAP.md § Computer use` for the verb shapes and
`docs/SAFETY.md § Computer-use sandbox` for the safety contract.

The two roles are independent. They share no IPC and no approval
state. A project that has Plume's computer-use turned off can
still be driven by an external computer-use agent through OS
accessibility, and vice-versa.

### UI contract for the EMITTING role

When the track lands, the chat panel grows a visible
**computer-use session area** with the same accessibility
expectations as the rest of Plume's UI:

- A session header naming the current target (`sandbox` /
  `host: <bundleId>`), the active `targetAllowlist`, and a
  visible Pause / Stop button. Pause has accessible label `Pause
  computer-use session`; Stop has `Stop and close
  computer-use session`.
- A trace list with `role="log"` and `aria-live="polite"` so
  screen readers and inbound agents are notified as each step
  appends. Each row carries the action kind (`click`, `type`,
  `scroll`, `drag`, `capture`, `observe`), the resolved target,
  the coordinates or text length, and the resulting status
  (`executed` / `rejected` / `pending-approval`). Rejected rows
  carry the typed reason (`Blocked: target not in allowlist`).
- A live screenshot pane fed by `computer.frame` events. The
  pane is `role="img"` with an accessible name pinned to "Current
  computer-use frame: <target>". Agents that can't read images
  fall back to the trace and the `computer.observe` AX-tree
  output.
- A session-end review surface that opens automatically on Stop
  or on `computer.session.end`. It re-renders the full trace
  with a "save trace" affordance — the trace stays available to
  the user even after the session ends.

The session area is gated by the same trust + project-open state
that gates chat: an untrusted project never reaches the session
start dialog, no matter what the model emits. The approval
dialog itself follows the same visible-trust convention as the
project-trust prompt — it's a foreground dialog with the target
named in plain text, no "remember this" toggle, and a focused
Approve / Reject button pair.

The trace surface is also exposed via `computer.trace` (read-only
IPC). That makes the EMITTING role auditable by the RECEIVING
role: an external operability agent driving Plume can verify
which actions the computer-use session has executed, the same
way the user does, without needing access to a hidden audit log.

### Safety boundary stays visible

The emitting track does NOT add a private automation-only path:

- Every computer-use action announces in the trace before it is
  considered "executed."
- The Pause / Stop controls are real buttons with accessible
  labels — the user (or an inbound operability agent) can stop a
  runaway session through the same path.
- There is no codepath that bypasses the session approval
  dialog. A session approved for `sandbox` cannot escalate to
  `host` without the full approval cycle for the host target.

If an external computer-use agent (running inbound) wants to
drive a Plume computer-use session (outbound), it does so by
clicking the visible Approve button in the session dialog —
exactly the same path a human would use. Approve actions are
not pre-granted by the operability surface.

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
