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
content from outside. The intent is to allow pure intra-project hard
links (uncommon, but legal) and reject any file whose inode is also
linked from outside the project root.

**v1 implementation is conservative.** `safety::path::ensure_no_hardlink_alias`
rejects any regular file with link count > 1, including links whose
sibling aliases are also inside the project. Distinguishing intra-project
from external aliases requires walking the filesystem to enumerate other
hard links to the same `(st_dev, st_ino)`, which is portable but
expensive. The conservative reject is the safe default and the false
positives (legitimate intra-project hardlinks) are rare in editor source
trees. Refining to the spec'd behavior is reserved for a hardening pass
before `fs.read` ships and is allowed to surface hardlinks at all.

### `.git/` writes

Reads under `.git/` are restricted to whitelisted files (`HEAD`,
`config`, `refs/**`, `index`) and pass through the redactor like any
other read. Writes under `.git/` are forbidden through `fs.*`; all
mutation goes through the `git` module's typed commands so we never let
the model invent bytes inside the repo metadata.

## Patch validation

Patches arrive as unified diffs from the model. Before applying:

- Parse the diff. Reject malformed hunks rather than guessing.
- For every file in the diff, run path-safety and ensure the path
  is inside the project root.
- Reject patches that touch files outside the user-approved scope for
  the current task.
- Reject patches whose pre-image does not match disk (avoids silent
  drift).
- Show the validated diff to the user. Apply only on explicit approval.
- Take a git checkpoint before applying multi-file patches in agent mode.

### D16: `patch.validate` ships the read-only half

The first two bullets above (parser + project-root path safety)
land as a real IPC verb in D16. `patch.validate(payload: { diff })`:

- Accepts the assistant's raw reply (fenced block + prose around
  it) or a bare unified diff. The parser strips a fenced
  ```diff/```patch block when present.
- Walks `--- ` / `+++ ` header pairs; strips `a/` / `b/` prefixes
  and tab-separated timestamps; detects create / delete via
  `/dev/null`; detects rename via differing header paths or git's
  `rename from` / `rename to` markers.
- Counts `@@` hunk headers per file; rejects file groups that
  have headers but no hunks (`noHunks`).
- For every diff-side path: lexical reject for absolute paths,
  `..` components, NUL bytes, empty strings (`absolutePath` /
  `pathEscape`). Then ancestor canonicalize — walk up from the
  joined path until an on-disk path is found and run
  `safety::path::ensure_inside` on that. This catches both
  modify / delete diffs targeting symlinked-out files AND
  create-diffs that target a missing file inside a symlinked-out
  parent (`link/new.rs` where `<root>/link -> /tmp/outside`).
  Create-diffs against genuinely-missing paths whose ancestors
  stay inside the project are permitted — refusing them would
  mean `patch.validate` could never green-light a new-file diff.
- Returns structured outcomes IN-BAND on `ok: false`. The `Promise`
  only rejects for the IPC envelope (`Version`) or for trust
  gating (`NeedsApproval` — no trusted project open, since path
  safety needs a root).

What `patch.validate` deliberately does NOT do today:

- It does NOT touch disk. The validator never reads file content;
  it only consults `Path::exists` to decide whether to layer the
  symlink-escape check on top of the lexical one.
- It does NOT verify pre-image hunks match disk. Two reasons:
  (1) D16 doesn't apply patches anyway, so a pre-image mismatch
  isn't dangerous; (2) reading file content for compare would
  cross into prompt-read territory and trigger the
  secret-redactor design questions that belong to `patch.apply`.
- It does NOT enforce any per-task `fileAllowlist`. There is no
  scoped-edit mode shipping yet, so the allowlist concept doesn't
  exist as a runtime input today.
- It does NOT call a model.

The Apply button on the rendered diff stays disabled even when
`patch.validate` returns `ok: true` — passing the validator means
"the diff is parseable and stays inside the project," not "Plume
will write this to disk." See `docs/IPC_CONTRACT.md § patch` for
the wire shape and `docs/IPC_ROADMAP.md` for the
`patch.apply` / `patch.checkpoint` / `patch.revert` verbs still
on the roadmap.

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

Agent autonomy is two independent axes plus explicit allowlists. The
old single "Stage 1–4" ladder conflated *what the model is allowed to
do* with *when the user is asked* — they are not the same thing, and
keeping them on one dial means "agent loop" silently implies "less
asking."

| Axis               | Values                                                    |
| ------------------ | --------------------------------------------------------- |
| `agentMode`        | `chat`, `propose-diff`, `scoped-edit`, `agent-loop`       |
| `approvalPolicy`   | `ask-each`, `ask-on-write`, `ask-on-fail`                 |
| `fileAllowlist`    | absent (no writes), or explicit list under `project_root` |
| `commandAllowlist` | absent (no commands), or explicit list of approved argv   |

`agentMode` and `approvalPolicy` are independent. An agent loop can
still prompt on every write; a chat session can run with `ask-on-write`
even though it never writes. Tier defaults pick a sane combination, but
the user can re-cross any of them.

### `agentMode`

| Mode           | The model can                                                                    |
| -------------- | -------------------------------------------------------------------------------- |
| `chat`         | Read attached/visible code; produce text answers                                 |
| `propose-diff` | The above, plus emit a unified diff for the user to review and apply. **D15 shipped the "emit" half: the chat panel renders the diff with per-line coloring and surfaces a *disabled* Apply button. D16 layered a read-only `patch.validate` IPC on top: the panel runs the model's reply through a parser + path-safety check and shows a `valid diff · N files · M hunks` or `invalid diff: <reason>` pill under the rendered diff. No IPC verb writes to disk on behalf of a diff today — the Apply button stays disabled even when validation passes.** |
| `scoped-edit`  | The above, plus apply patches inside `fileAllowlist` and run commands inside `commandAllowlist`, each gated by `approvalPolicy` |
| `agent-loop`   | The above, plus iterate read/edit/test/fix until the iteration cap, an abort, or `Stop` |

### `approvalPolicy`

| Policy         | Meaning                                                                                          |
| -------------- | ------------------------------------------------------------------------------------------------ |
| `ask-each`     | Every tool call prompts, even ones whose argv is already in the ledger                           |
| `ask-on-write` | Read-only tools run silently. Writes and shell commands prompt unless the exact normalized argv is already in the approval ledger |
| `ask-on-fail`  | Same as `ask-on-write` for first-time runs. Additionally, when an already-ledger-approved argv exits non-zero and the agent wants to retry the same argv (or run a previously approved follow-up), the retry runs without re-prompting. **Never grants first-run permission.** **Never applies to writes or shell commands without a ledger entry.** Outside the verifier-retry case it behaves like `ask-on-write`. |

There is no `never` policy. There is no bypass mode. The agent is never
allowed to decide on its own that an action is safe enough to skip the
ledger.

The verifier-retry case is the only thing `ask-on-fail` exists for:
`npm test` was approved, the agent edits a file, the agent re-runs
`npm test`. Without `ask-on-fail` the user re-approves on every loop
iteration, which is friction that doesn't earn its keep.

### Tier defaults

The model picker (see `docs/MODEL_PROVIDERS.md`) maps capability tiers
to default combinations of these axes:

| Tier                | Default `agentMode` | Default `approvalPolicy` | Allowlists                                      |
| ------------------- | ------------------- | ------------------------ | ----------------------------------------------- |
| Tiny / Fast         | `chat`              | `ask-each`               | none                                            |
| Small / Useful      | `propose-diff`      | `ask-each`               | none                                            |
| Medium / Capable    | `scoped-edit`       | `ask-on-write`           | per-task explicit allowlist                     |
| Large / Workstation | `agent-loop`        | `ask-on-fail`            | per-task allowlist + iteration cap + checkpoint |

Defaults exist so a small local model is not handed `agent-loop` with
`ask-on-fail` by accident. The user can re-cross from the picker.

### `agent-loop` always requires

- An explicit `fileAllowlist`.
- An explicit `commandAllowlist`.
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
- Allow Plume to read git state by spawning `git` against the repo.
- (Later, with a separate prompt) Allow Plume to run detected commands.

The git item is grouped with trust because `git status` and
`git rev-parse` execute repo-local hooks and any configured
`core.fsmonitor` binary. Trust is the gate; until the user confirms,
Plume reports `ProjectMeta.git = null` even on a real repo.

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
  content is never sent to a model. The check lives in
  `fs::policy::block_reason` and is shared between display reads
  (`fs.read`) and prompt reads (`prompts::read::read_for_prompt`).
- **Content-pattern redaction.** Every file the prompt assembler reads
  passes through `prompts::redact` (D8). The redactor is the only
  producer of `RedactedContent`; raw file bytes never leave the
  `prompts` module. Each match is replaced with a `[REDACTED:<kind>]`
  marker so the model sees that a secret was there without learning
  its length or contents.

  Patterns shipped in D8:
  - AWS access keys (`AKIA` + 16 chars `[A-Z0-9]`) → `aws-key`.
  - GitHub PATs (`ghp_` + ≥ 36 alnum, or `github_pat_` + ≥ 20
    alnum/underscore) → `github-pat`.
  - OpenAI / Anthropic-style keys (`sk-` + ≥ 20 chars
    `[A-Za-z0-9_\-]`, covering `sk-…` and `sk-ant-…`) → `api-key`.
  - Three-segment JWTs starting with `eyJ` → `jwt`.
  - Case-insensitive `Bearer <token>` headers (≥ 8 token chars) →
    `bearer`.

  Patterns roadmap (matched in `safety::secrets.rs` when the
  command sandbox lands; not yet implemented):
  - Connection strings with embedded passwords
    (`scheme://user:PASSWORD@host`). Deferred because matching
    them safely requires a small URL parser to avoid mangling
    `https://` URLs without credentials.
  - Hex API keys / generic high-entropy strings. Trade-off: high
    false-positive rate against random-looking but legitimate
    identifiers. Worth revisiting once the command-runner output
    redactor needs a generic backstop.

- **Display-read attachment cap.** Display reads (`fs.read`) cap at
  2 MiB so the editor can show a large lockfile; prompt reads cap
  at 256 KiB (`PROMPT_READ_MAX_BYTES`) because the content goes
  into a model context window.
- **Log and command output.** The same redactor will run over
  command output before it reaches the UI or the model when the
  command sandbox lands. Today's scope is prompt-read attachments
  (D8/D10), the auto-included project instructions (D11), and the
  D12 read-only context preview.
- **Project instructions read path (D11).** When a trusted
  project has a root `AGENTS.md`, the chat handler folds it in
  as a `system` message on every send. The reader is the same
  `prompts::read::read_for_prompt` used for file attachments —
  same secret-filename block, 256 KiB cap, binary detection,
  hardlink alias check, and content redactor. Errors don't
  propagate: a broken `AGENTS.md` skips silently rather than
  failing the user's chat. The frontend's "Project instructions
  included" indicator is driven by `ProjectMeta.hasAgentsMd`
  (presence check) plus the per-send `instructionsIncluded`
  field on `ChatSendStartedResponse` (actual confirmation).
- **Context preview path (D12).** `chat.context` runs the same
  Rust-private `prompts::read_for_prompt` + `prompts::redact`
  pipeline as `chat.send` — same secret-filename block, 256 KiB
  cap, binary detection, hardlink alias check, line-range
  validation. The preview returns only the summary numbers
  (`originalBytes`, `redactionCount`, source filename); raw file
  bytes never cross IPC. Attachment rejections that the real
  send would raise as a typed `IpcError` surface here as
  `attachment.status === 'blocked'` with a stable `reason`
  code, so the UI can show the user "this would be blocked
  because of X" without the backend leaking the file contents
  that triggered the rejection.
- A user override (per-file, per-session) is deferred until there
  is a concrete use case; today's shipping behavior is "the
  redactor always runs."
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
