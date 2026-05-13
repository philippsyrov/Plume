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

## Computer-use sandbox (post-MVP)

Plume's computer-use track is the **EMITTING** side: the model
asks Plume to drive a target environment on the user's behalf —
clicks, types, scrolls, captures screenshots, optionally reads
an accessibility tree. This is a separate axis from
`docs/AGENT_OPERABILITY.md`, which is about external agents
driving Plume's UI through ordinary OS accessibility. The two
share no IPC: operability rides on platform accessibility APIs
that target Plume; computer-use is a `computer.*` tool family
Plume exposes to the model through its own IPC layer.

Nothing in this section ships today. It defines the contract a
future slice has to meet.

### Two-phase target boundary

The track lands in two phases so the blast radius grows
deliberately:

1. **Phase A — bundled webview sandbox.** Plume opens a webview
   it controls inside its own window. The sandbox enforces a
   strict CSP, has no disk access, has no network unless the
   session whitelists hosts, and cannot navigate to arbitrary
   URLs. The "computer" the model drives is entirely Plume's
   territory — input synthesis, screenshots, and DOM observation
   all stay inside the webview's own boundary.
2. **Phase B — host desktop.** Plume drives the user's actual
   macOS desktop using accessibility APIs + `CGEvent` input
   synthesis + `CGWindowList` screen capture. **Off by default.**
   Enabling it requires three gates that sit at different
   layers and DO NOT collapse into each other:
   1. The project is trusted (project-level, persistent in
      Plume's trust ledger).
   2. macOS has granted Plume the Accessibility and Screen
      Recording entitlements. These are **app-level persistent
      grants** managed in System Settings → Privacy & Security.
      macOS prompts the user once when Plume first attempts
      either, then remembers the choice across launches and
      sessions until the user revokes it. Plume does NOT
      control when this OS prompt fires and cannot make it
      per-session.
   3. Plume's own per-session approval dialog (the one Plume
      renders on every `computer.session.start`), which names
      the target and the allowlist. **This** is the gate
      that does not persist: granting it for one session does
      NOT grant it for the next, and there is no persistent
      "always allow host" toggle on this layer.

   Revoking the OS-level grant in (2) disables Phase B regardless
   of any prior session-level approval. Approving (3) on top of
   a missing (2) does not unlock host access — Plume must
   reject with a typed `Blocked` error naming the missing
   permission and prompt the user to enable it in System
   Settings.

There is no codepath from a Phase A approval to Phase B execution.
Phase A approvals scope to `targetKind: 'sandbox'`; Phase B is a
separate target with its own dialog.

### Per-session approval, no persistent ledger

Computer-use approvals are **session-scoped only**. They do not
land in `<project>/.plume/approvals.toml`. The argv-approval
model that gates `commands.run` makes sense there because a
shell command is a well-defined identity (normalized argv). A
computer-use SESSION has no equivalent: the same target can be
asked to do wildly different things between two sessions, and a
"this app was approved last week" gate would invite the user to
click through without re-reading what's being requested today.

Concretely:

- Every `computer.session.start` shows a foreground approval
  dialog naming the `targetKind`, the resolved target (sandbox
  URL or host app's bundleId / window title), and the requested
  `targetAllowlist`.
- The dialog has no "remember this" checkbox. The user re-reads
  every session.
- The dialog is gated by project trust — an untrusted project
  cannot reach the dialog at all.

### Target allowlist

Every session carries an explicit `targetAllowlist` — a list of
the URLs, bundleIds, or window titles the session is permitted
to interact with. Actions whose resolved target is outside the
list reject with `Blocked`. Wildcards (`*`, `**`, regex) are not
accepted entries; the list is exact-match only.

For Phase A:

- Default allowlist is the bundled sandbox URL (e.g.
  `plume-sandbox://blank`). Navigating elsewhere requires the
  user to add hosts to the list when starting the session.

For Phase B:

- Default allowlist is empty — even with Phase B opted in, a
  session that doesn't name a specific target has nowhere to
  click.
- An allowlist entry of "the whole desktop" is not a valid
  entry. The user names individual apps (bundleId) or specific
  windows; a session that wants to span apps must name each one
  and the dialog renders each.

### Visible trace

Every action is recorded in a visible trace area in the chat
panel. The trace is:

- **Append-only during the session.** Each `computer.action`
  event appends a step with timestamp, action kind, coordinates
  (or text length / scroll delta), the target the action
  resolved against, and the resulting status (`executed` /
  `rejected` / `pending-approval`).
- **Pausable and stoppable.** A visible Pause button suspends
  action dispatch — pending model-emitted actions wait until
  the user resumes; a Stop button ends the session, runs
  `computer.session.end`, and clears any in-flight handles.
- **Surfaced in `computer.trace`.** The trace is also an IPC
  read so an agent driving Plume's UI (the operability story —
  the other side of the wall) can verify what the
  computer-use session has done. The two surfaces meet *here*:
  Plume's outbound computer-use is auditable by Plume's inbound
  agent-operability.

### No hidden host control

Outside the explicit `computer.*` verbs there is no other path
from the model into host input synthesis. The chat surface does
not "secretly" route arbitrary text into a focused window. The
prompt-read pipeline does not write. There is no shell-out for
"open the browser to this URL" that bypasses the approval
dialog.

### Redaction before model sees frames

The prompt-read redactor (`§ Secret handling`, above) is
text/regex-based. It rewrites secret-shaped substrings in a
`String`. **It cannot rewrite the pixels of a screenshot.** A
secret-shaped substring painted into a PNG stays painted. Be
honest about which surfaces the redactor protects and which
ones it does not:

- **Image bytes** (`computer.capture` → PNG payload): NOT
  redacted by the existing text-regex redactor. Image-side
  safety rests on (1) scaling / cropping the captured region
  to what the model actually needs, (2) the mandatory
  `targetAllowlist` (a session that captures its own
  user-named target is the approved outcome, not a leak), and
  (3) Phase A's option to honour a per-session DOM filter
  (drop password inputs from the rendered DOM before capture).
  A session that points its capture at a password manager will
  send the model a screenshot of that password manager — the
  user named the target.
- **Text derived from a capture** (`computer.observe` →
  accessibility / DOM tree; future OCR text from a screenshot;
  DOM string contents pulled from Phase A): DOES pass through
  the existing prompt-read redactor before the model sees it.
  The same `AKIA…` / `ghp_…` / `sk-…` / JWT / `Bearer …`
  patterns are masked exactly as they are for file
  attachments.

The split matters because the user needs to understand which
operation is safe at the pixel level (it isn't, by design —
the safety story is consent + scope, not regex) and which one
is safe at the text level (it is, via the existing redactor).
An image-aware redactor (blur over OCRed high-entropy regions,
DOM-aware password-field masking before capture, etc.) is not
on the roadmap — the v1 contract is "approve the target,
scale/crop the bytes, redact extracted text."

### Failure modes that must reject

- A session that requests Phase B host access from an untrusted
  project — reject before the dialog renders.
- A session that requests host access without macOS accessibility
  / screen-recording permissions — reject with a typed `Blocked`
  error explaining which permission is missing.
- An action whose target resolved outside the `targetAllowlist`
  — reject with `Blocked` and append a `rejected` row to the
  trace.
- A session that idles past its configured timeout — auto-stop
  and surface the timeout in the trace.

### Open questions

- Whether the host-track Phase B backend integrates an upstream
  reference like `trycua` / `cua-driver` (https://github.com/trycua)
  or implements the platform-API calls directly. Today: neither
  is wired, no dependency added, no install required. The doc
  shape leaves room for either choice.
- Whether `computer.observe` lives behind its own capability flag
  (it can leak target content even when no click happens). Today's
  answer: probably yes — listed as its own approval bullet on the
  session dialog.

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
