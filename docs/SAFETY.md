# Safety

Plume's safety model has three layers: file sandbox, command sandbox, and
agent staging. The Rust backend enforces all three. The frontend can
request side effects but cannot perform them.

## File sandbox

- A single `project_root` is set when the user opens a folder.
- Every read and write resolves the requested path via
  `safety::resolve(project_root, requested)`, which:
  1. Canonicalizes the path.
  2. Rejects anything outside `project_root`.
  3. Rejects symlinks that escape the project after resolution.
  4. Rejects paths matching the deny list (`.git/objects/**`, `.env*` for
     reads only with redaction; never silently for writes).
- `.plume/` inside the project is allowed for app-managed files (logs,
  registry cache).

### Avoid TOCTOU

A canonicalize-then-open sequence is racy: a symlink swap between the
check and the open can defeat the check. Backend implementations must
either:

- open the file once via the canonicalized path and operate on the
  resulting file descriptor (`openat`-style on Unix; an equivalent on
  Windows), or
- canonicalize and operate atomically with no async point in between.

Helper functions that take a `Path` and re-resolve internally are
banned in safety-sensitive paths.

### Hard links

Hard links are not addressed by symlink resolution — a hard link inside
the project pointing at `/etc/passwd` is "inside" by path but reads
content from outside. `safety::resolve` must reject any file whose inode
link count is greater than one and whose other links live outside the
project root. Pure intra-project hard links (uncommon, but legal) are
allowed.

### `.git/` writes

Reads under `.git/` are restricted to whitelisted files (`HEAD`,
`config`, `refs/**`, `index`) and pass through the redactor like any
other read. Writes under `.git/` are forbidden through `fs.*`; all
mutation goes through the `git` module's typed commands so we never let
the model invent bytes inside the repo metadata.

## Patch validation

Patches arrive as unified diffs from the model. Before applying:

- Parse the diff. Reject malformed hunks rather than guessing.
- For every file in the diff, run `safety::resolve` and ensure the path
  is inside the project root.
- Reject patches that touch files outside the user-approved scope for
  the current task.
- Reject patches whose pre-image does not match disk (avoids silent
  drift).
- Show the validated diff to the user. Apply only on explicit approval.
- Take a git checkpoint before applying multi-file patches in agent mode.

## Command sandbox

- Plume detects candidate verification commands from project files
  (`package.json` scripts, `Cargo.toml`, `pyproject.toml`,
  `scripts/verify.sh`).
- Each command requires explicit approval the first time. Approvals are
  scoped per-project and persisted in the approval ledger.
- A small built-in deny pattern blocks obviously destructive shells
  (`rm -rf /`, `sudo`, package-manager-uninstalls, `git push --force`,
  `:(){:|:&};:`, etc.). The pattern list lives in `safety::commands.rs`
  and is easy to audit.
- Commands run in a child process scoped to the project root. Output is
  streamed to the UI line by line. Output passes through the secret
  redactor before being shown to the model.

### Argv normalization

Approvals are keyed by **normalized argv**, not raw command strings.
Normalization rules:

- `argv` is a `Vec<String>`, not a single shell string. The frontend
  never sends a string to be re-split by the shell.
- The first element is resolved against `PATH` once and stored as a
  basename plus an absolute path; both are required to match on later
  runs.
- Trailing args are kept verbatim. `npm test` and `npm test --watch`
  are different approvals. Watch flags should be rare and approved
  explicitly.
- Environment-mutating wrappers (`env A=1 npm test`) are rejected; use
  the wrapper's own approval if needed.

## Approval ledger

Stored at `<project>/.plume/approvals.toml` (project-local, gitignored).
One record per approved argv:

```toml
[[approval]]
argv = ["npm", "test"]
binary = "/opt/homebrew/bin/npm"
approved_at = 2026-05-03T10:14:00Z
expires_at = 2026-08-01T00:00:00Z   # 90-day default; null = no expiry
approved_by = "user"                # 'user' today; 'agent' reserved
```

- Default expiry is 90 days; the user can override per record.
- Revoke via the approvals UI; the ledger is human-readable for manual
  edits.
- A stale-binary mismatch (the resolved absolute path no longer matches
  the recorded one) re-prompts for approval; it does not auto-update.

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
- A one-key abort (wired to `chat.cancel` + `commands.cancel`).

## Project trust

The first time Plume opens a folder it shows a trust prompt summarizing
what Plume found (`AGENTS.md`, `package.json` scripts,
`scripts/verify.sh`, etc.) and asks the user to confirm:

- Treat this folder as a project root.
- Allow Plume to read `AGENTS.md` and other instruction files.
- (Later, with a separate prompt) Allow Plume to run detected commands.

A repo's instruction file can guide Plume but cannot grant Plume new
permissions on its own. Approval lives with the user.

## Project-root lifecycle

Opening a new project root in the same window:

1. Cancels every in-flight `ChatStreamId` and `RunHandle`.
2. Stops any provider server Plume owns for the previous project.
3. Drops the previous session transcript from memory; persistence is
   per-project so transcripts do not bleed across.
4. Re-runs the trust prompt.

Multiple Plume windows on different projects are isolated processes.
They share no in-memory state. They may share an OS-level provider
daemon (e.g. `ollama serve`) but never share an approval ledger or
session.

## Secret handling

- **Filename-pattern redaction.** Reads of files matching `.env*`,
  `id_rsa`, `*.pem`, `*.key`, `*credentials*`, `*token*` are blocked
  from prompt context entirely; their existence is acknowledged but the
  content is never sent to a model.
- **Content-pattern redaction.** Every file the prompt assembler reads
  passes through a redactor that masks the matched span before the
  prompt is built. Patterns live in `safety::secrets.rs` and at minimum
  cover:
  - AWS access keys (`AKIA[0-9A-Z]{16}`)
  - GitHub PATs (`ghp_`, `github_pat_`)
  - OpenAI/Anthropic-style keys (`sk-[A-Za-z0-9]{20,}`)
  - Generic JWT triplets
  - Common `Authorization: Bearer` headers in test fixtures
  - Connection strings with embedded passwords
- **Log and command output.** The same redactor runs over command
  output before it reaches the UI or the model.
- A user can override per-file, per-session, with an explicit toggle.
- Plume does not upload anything by default. Cloud providers are a
  separate, labeled mode.

## Threats Plume defends against

- **Model suggests a destructive shell command.** Mitigation: command
  approval + deny list.
- **Model writes outside the project root.** Mitigation: path
  canonicalization in `safety::resolve` + FD-based ops.
- **Symlink/hardlink escape.** Mitigation: link-aware resolver above.
- **Prompt injection from a file in the repo.** Mitigation: instruction
  files are context, not commands; approvals stay with the user.
- **Secret leakage into a prompt or log.** Mitigation: filename + content
  redactor on every prompt build and every log line.
- **Accidental cloud call.** Mitigation: cloud providers are explicit,
  off by default, and visibly labeled in the status strip.
- **Malicious repo instructions.** Mitigation: trust prompt on every new
  project; permissions never granted by repo content.

## Reverting

Every Plume-initiated edit is logged with:

- Timestamp.
- Provider + model.
- The applied diff.
- A pointer to the pre-edit git checkpoint when one exists.

"Undo last Plume edit" reverts the diff and refreshes git status.
