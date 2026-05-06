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

Dev-mode Tauri binaries are not always discoverable by macOS accessibility
tools because `tauri dev` runs a raw binary instead of an installed app
bundle. That is fine for manual development, but not enough for Plume's
agent-operability goal.

Plume should eventually have a smoke-test launch path that opens a real
local app bundle with logs visible:

```bash
./scripts/smoke-app.sh
```

Reserved behavior:

- Build or locate a local `Plume.app` bundle.
- Launch it with dev/smoke configuration.
- Keep logs visible to the developer.
- Avoid network/model downloads.
- Create only ignored smoke fixtures.
- Clean up smoke fixtures after the run.

The smoke path should let computer-use agents test the actual desktop
window: open a project, trust it, browse files, open CodeMirror, trigger
blocked-file behavior, and close the project.

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

