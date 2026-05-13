# Smoke Testing

This checklist proves the packaged desktop app works through the same
visible UI a human or computer-use agent drives. Use it after UI,
IPC, Tauri config, CSP, capability, bundle, or safety-policy changes.

## What It Catches

`npm run tauri dev` is useful for fast iteration, but it runs a raw
debug binary. The smoke path builds and launches a real macOS
`Plume.app`, so it catches production-bundle problems that dev mode can
hide.

The first real agent smoke caught exactly that class of bug: the app
bundle was addressable by macOS, but frontend IPC was blocked until
Plume added:

- `src-tauri/capabilities/default.json`
- a stable window label of `main`
- CSP support for `ipc:` and `http://ipc.localhost`

Keep this test around because it checks the trust/read/browser path and
the Tauri production IPC bridge at the same time.

## Pre-Flight

```bash
git status --short --branch
./scripts/verify.sh
```

Expected:

- You are on the branch under test.
- No unrelated dirty files.
- The verifier passes, or any warning is understood before smoke starts.

## Fixture

Create a fake secret file for the blocked-read check:

```bash
printf 'API_KEY=plume-smoke-test-not-real\n' > .env.smoke
```

Do not use a real secret. Remove the file before finishing:

```bash
rm -f .env.smoke
```

## Launch

```bash
./scripts/smoke-app.sh
```

Expected:

- Builds with `CARGO_NET_OFFLINE=true`.
- Produces `src-tauri/target/debug/bundle/macos/Plume.app`.
- Quits any previous instance of that exact bundle.
- Launches `Plume.app`.
- macOS / computer-use can target `Plume` or bundle id `dev.plume.app`.

Known warning:

- Tauri currently warns that `dev.plume.app` ends with `.app`. This is
  parked as a follow-up rename to a safer id such as
  `dev.plume.desktop`.

## Visual Checklist

Drive the app through visible clicks/keyboard, not hidden IPC.

> TODO: step 1 currently hard-codes Philipp's local checkout path. Swap
> for a `<plume-repo>` placeholder when this repo is depersonalized.

| Step | Action | Expected |
| --- | --- | --- |
| 1 | Open `/Users/philippsyrov/Desktop/CS Projects/Plume` | Project opens. If already trusted, status strip shows `trusted`, git branch, dirty count, and `npm`. |
| 2 | Trust prompt, when shown | Click `Trust this project`; status strip replaces the trust panel. |
| 3 | Open `docs/BOOTSTRAP.md` | CodeMirror shows text with line numbers and monospace code font. |
| 4 | Open `src-tauri/icons/icon.png` | Binary placeholder appears; bytes are not rendered as text. |
| 5 | Open `.env.smoke` | Red blocked message appears: `.env.smoke is blocked by the secret-filename policy`. |
| 6 | Open `package-lock.json` | JSON renders in CodeMirror and scrolls. |
| 7 | Provider panel, when Ollama or LM Studio is running | Each model row carries a `Select` button. Offline/`not configured` providers render their rows with `Select` disabled. |
| 8 | Click `Select` on a model row | Center "Selected model" banner shows `<Provider> · <model id>` and a Clear button; the picked row gains a `✓ selected` badge. |
| 9 | If Ollama: expand a model first, then click `Select` | Banner additionally renders the fit verdict badge captured at click time. |
| 10 | With no model selected, look at the Chat panel | Prompt input is disabled with placeholder "Pick a model on the left to enable chat."; status reads "No model selected." |
| 11 | With a non-Ollama model selected (e.g. LM Studio) | Chat input is disabled with placeholder pointing at the Ollama-only limit; status reads "Selected provider has no chat adapter yet (Ollama only in D7)." |
| 12 | With an Ollama model selected and the daemon up, type a short prompt and click `Send` | Send button is replaced by a `Stop` button; an in-progress assistant entry with a blinking cursor appears in the transcript and gains tokens as they stream in. On completion the cursor disappears and the entry shows `served by <model>` + duration, plus a D9 stats footer line: `<n> tokens · <r> tok/s`. Hovering the stats line surfaces the prompt-eval breakdown. |
| 13 | With an Ollama model selected, type a longer prompt (e.g. "write a short essay about clouds"), click `Send`, then click `Stop` mid-stream | Stream stops within ~1 second; the partial reply stays in the transcript with a `stopped by you` meta line; the input returns to ready. |
| 14 | Without ollama running, with an Ollama model selected (use stale picker state) | Send produces a red error row in the transcript with a `could not reach ollama` message; the panel returns to ready, not stuck on `Sending…`. |
| 15 | D8 attach: with `docs/BOOTSTRAP.md` open in the inspector, click `Attach current file` on the chat panel | Chip appears showing `docs/BOOTSTRAP.md` + size; the attach button label flips to `Replace with current file`. |
| 16 | D8 attach send: type "summarise this file in one sentence" and `Send` | Stream emits a reply that references the attached file's contents; the visible transcript shows the user turn with the chip inline. Chip clears from the form bar after send. |
| 17 | D8 attach clear: open `package-lock.json`, click `Attach current file`, then click `×` on the chip before sending | Chip disappears. Sending a new prompt now produces a reply that does NOT reference the file's contents. |
| 18 | D8 secret block: open `.env.smoke` (still blocked by display reads from step 5), try to attach via the chat panel | Attach button stays disabled because the inspector reports a blocked read — the chat panel never reaches the prompt-read path for an .env file. |
| 19 | D8 binary block: open `src-tauri/icons/icon.png`, look at the chat panel | Attach button is disabled with the hint "Binary files cannot be attached as text context." |
| 20 | D10 selection attach: with `docs/BOOTSTRAP.md` open, select a few lines (e.g. lines 8–14) in the editor | Attach button label flips to `Attach selection`; hint reads `Inspector has lines 8–14 of docs/BOOTSTRAP.md selected.` |
| 21 | D10 selection send: click `Attach selection`, then type "explain just these lines" and Send | Chip shows `docs/BOOTSTRAP.md:8–14`; the user turn in the transcript renders the same chip. The reply discusses only those lines (model behavior; close enough is a pass). Chip clears after send. |
| 22 | D10 single-line: select exactly one line in the editor, click `Attach selection` | Chip renders `docs/BOOTSTRAP.md:N` (single line form, no en-dash). Clear with `×`. |
| 23 | D10 deselect: click somewhere in the editor to collapse the selection | Attach button reverts to `Attach current file`. Sending now sends the whole file (no `startLine`/`endLine` on the wire). |
| 24 | D11 instructions indicator before first send: a Plume-like project (with `AGENTS.md` at root) shows a `¶ AGENTS.md available` badge in the chat header. Hover for the tooltip. | Badge visible next to the `read-only` badge; tooltip says "will be folded in on your next send". Subtitle mentions AGENTS.md will ride along. |
| 25 | D11 instructions confirmation: send any prompt with `AGENTS.md` present. After the stream completes the badge label flips to `¶ AGENTS.md included` with a "backend confirmed" tooltip. The subtitle also updates to past tense. | Badge label changes after the first accepted send. |
| 26 | D11 instructions effect: ask a project-specific question like "what is rule #1 in this project?" | Reply quotes or references the relevant AGENTS.md content (model-behavior dependent — close enough is a pass). If you temporarily rename `AGENTS.md` (without re-opening the project) and re-ask, the model loses that context AND the badge flips to `¶ AGENTS.md skipped` (warn-colored): the backend reports `instructionsIncluded=false` while `ProjectMeta.hasAgentsMd` is still cached as true. Restore the filename or re-open the project to clear that state — if you re-open with no AGENTS.md on disk, the badge disappears entirely on the next refresh. |
| 27 | D12 context preview baseline: open a project with `AGENTS.md` at root, do NOT attach a file | "Will ride along:" row appears between the attach bar and the textarea. A single `¶ AGENTS.md · <bytes>` chip is visible (hover tooltip shows redaction count). With no AGENTS.md the row is hidden entirely. |
| 28 | D12 context preview ready attachment: attach a small text file (e.g. `docs/BOOTSTRAP.md`) | A second `¶ docs/BOOTSTRAP.md · <bytes>` chip appears next to AGENTS.md. Tooltip describes "read-only context." Both chips ride along on the next send. |
| 29 | D12 context preview selection range: select lines 5–10 of `docs/BOOTSTRAP.md` in the inspector, click `Attach selection` | Attachment chip in the preview flips to `¶ docs/BOOTSTRAP.md:5–10 · <bytes>`. Bytes reflect the WHOLE file (the preview reads the full file so the redactor sees lines outside the range). |
| 30 | D12 context preview blocked attachment: temporarily rename `.env.smoke` to live inside the project (e.g. `mv .env.smoke .env`), refresh navigator, try to attach via the chat panel | Attach button stays disabled (D8 already enforces this at the inspector level). For a direct test of the preview's blocked-path: call `chat.context` from devtools with `{ attachment: { kind: 'projectFile', relPath: '.env' } }` while `.env` exists — response shows `attachment.status === 'blocked'`, `reason === 'blocked'`, `message` mentions the secret-filename policy. Restore the fixture name afterwards. |
| 31 | Click `Clear` on the chat panel | Transcript empties; input returns to ready state. Preview stays visible (clearing transcript doesn't clear chip). |
| 32 | Click `Close` | App returns to the open form. |

## Report Format

Use a short table:

```text
Smoke result: PASS / FAIL

Open project: PASS
Trust: PASS / already trusted
BOOTSTRAP.md: PASS
icon.png binary placeholder: PASS
.env.smoke blocked: PASS
package-lock.json: PASS
Select model: PASS / N/A (no runtime up)
Chat disabled (no selection): PASS
Chat disabled (non-Ollama selection): PASS / N/A
Chat streamed reply (Ollama up): PASS / N/A
Chat stats footer (tokens + tok/s) renders: PASS / N/A
Chat Stop button cancels mid-stream: PASS / N/A
Chat ProviderDown surfaced: PASS / N/A
Attach current file chip appears: PASS / N/A
Attach + send round-trips file context: PASS / N/A
Attach × clears chip without sending: PASS / N/A
Attach disabled for .env (secret-filename): PASS / N/A
Attach disabled for binary file: PASS / N/A
Attach selection (multi-line range) round-trips: PASS / N/A
Attach selection chip shows single-line form for one-line picks: PASS / N/A
Collapsing the editor selection reverts to "Attach current file": PASS / N/A
Project instructions badge shows "available" before first send: PASS / N/A
Project instructions badge flips to "included" after send: PASS / N/A
Project instructions badge shows "skipped" after AGENTS.md rename without reopen: PASS / N/A
Model references AGENTS.md content on project-specific Q: PASS / N/A
Context preview shows AGENTS.md chip with bytes when present: PASS / N/A
Context preview adds an attachment chip when one is attached: PASS / N/A
Context preview shows line range when attaching a selection: PASS / N/A
Context preview surfaces blocked attachment with reason via devtools probe: PASS / N/A
Clear chat: PASS / N/A
Close: PASS
Fixture cleanup: PASS
Final git status: ...
```

If any step fails, keep the app and logs around until the failure is
understood. A hang on `Opening...` usually means the frontend IPC bridge
did not reach Rust, so check Tauri capabilities and CSP first.
