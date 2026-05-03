# Safety

Plume's safety model has three layers: file sandbox, command sandbox, and
agent staging. The Rust backend enforces all three. The frontend can request
side effects but cannot perform them.

## File sandbox

- A single `project_root` is set when the user opens a folder.
- Every read and write resolves the requested path via
  `safety::resolve(project_root, requested)` which:
  1. Canonicalizes the path.
  2. Rejects anything outside `project_root`.
  3. Rejects symlinks that escape the project.
  4. Rejects paths matching the deny list (`.git/objects/**`, `.env*` for
     reads only with redaction; never silently for writes).
- `.plume/` inside the project is allowed for app-managed files (logs,
  registry cache).

## Patch validation

Patches arrive as unified diffs from the model. Before applying:

- Parse the diff. Reject malformed hunks rather than guessing.
- For every file in the diff, run `safety::resolve` and ensure the path is
  inside the project root.
- Reject patches that touch files outside the user-approved scope for the
  current task.
- Reject patches whose pre-image does not match disk (avoids silent drift).
- Show the validated diff to the user. Apply only on explicit approval.
- Take a git checkpoint before applying multi-file patches in agent mode.

## Command sandbox

- Plume detects candidate verification commands from project files
  (`package.json` scripts, `Cargo.toml`, `pyproject.toml`,
  `scripts/verify.sh`).
- Each command requires explicit approval the first time. Approvals are
  scoped per-project and persisted.
- A small built-in deny pattern blocks obviously destructive shells
  (`rm -rf /`, `sudo`, package-manager-uninstalls, `git push --force`,
  `:(){:|:&};:`, etc.). The pattern list lives in `safety::commands.rs`
  and is easy to audit.
- Commands run in a child process scoped to the project root. Output is
  streamed to the UI line by line.

## Agent stages

The agent loop is staged. The model's autonomy grows only as the user
grants it. Tier defaults from `docs/MODEL_PROVIDERS.md` map to a stage.

| Stage | What the model can do                                      | Default for tier      |
| ----- | ---------------------------------------------------------- | --------------------- |
| 1     | Chat about visible code                                    | Tiny / Fast           |
| 2     | Propose unified diff; user applies                         | Small / Useful        |
| 3     | Edit files in an approved allow-list, run approved verify  | Medium / Capable      |
| 4     | Multi-step loop with task budget + iteration cap           | Large / Workstation   |

Stage 4 always requires:

- An explicit file allow-list.
- An explicit command allow-list.
- A maximum iteration count.
- A pre-run git checkpoint.
- A visible session log.
- A one-key abort.

## Project trust

The first time Plume opens a folder it shows a trust prompt summarizing
what Plume found (`AGENTS.md`, `package.json` scripts,
`scripts/verify.sh`, etc.) and asks the user to confirm:

- Treat this folder as a project root.
- Allow Plume to read `AGENTS.md` and other instruction files.
- (Later, with a separate prompt) Allow Plume to run detected commands.

A repo's instruction file can guide Plume but cannot grant Plume new
permissions on its own. Approval lives with the user.

## Secret handling

- Files matching `.env*`, `id_rsa`, `*.pem`, `*.key`, `*credentials*`,
  `*token*` are redacted from prompt context by default.
- Logs and session transcripts pass through the same redactor.
- A user can override per-file, per-session, with an explicit toggle.
- Plume does not upload anything by default. Cloud providers are a
  separate, labeled mode.

## Threats Plume defends against

- **Model suggests a destructive shell command.** Mitigation: command
  approval + deny list.
- **Model writes outside the project root.** Mitigation: path
  canonicalization in `safety::resolve`.
- **Prompt injection from a file in the repo.** Mitigation: instruction
  files are context, not commands; approvals stay with the user.
- **Secret leakage into a prompt or log.** Mitigation: redactor on every
  prompt build and every log line.
- **Accidental cloud call.** Mitigation: cloud providers are explicit, off
  by default, and visibly labeled in the status strip.
- **Malicious repo instructions.** Mitigation: trust prompt on every new
  project; permissions never granted by repo content.

## Reverting

Every Plume-initiated edit is logged with:

- Timestamp.
- Provider + model.
- The applied diff.
- A pointer to the pre-edit git checkpoint when one exists.

"Undo last Plume edit" reverts the diff and refreshes git status.
