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
| 15 | D8 attach: with `docs/BOOTSTRAP.md` open in the inspector, click `Attach current file` on the chat panel | Chip appears showing `docs/BOOTSTRAP.md` + size; the attach button label flips to `Replace attachment`. |
| 16 | D8 attach send: type "summarise this file in one sentence" and `Send` | Stream emits a reply that references the attached file's contents; the visible transcript shows the user turn with the chip inline. Chip clears from the form bar after send. |
| 17 | D8 attach clear: open `package-lock.json`, click `Attach current file`, then click `×` on the chip before sending | Chip disappears. Sending a new prompt now produces a reply that does NOT reference the file's contents. |
| 18 | D8 secret block: open `.env.smoke` (still blocked by display reads from step 5), try to attach via the chat panel | Attach button stays disabled because the inspector reports a blocked read — the chat panel never reaches the prompt-read path for an .env file. |
| 19 | D8 binary block: open `src-tauri/icons/icon.png`, look at the chat panel | Attach button is disabled with the hint "Binary files cannot be attached as text context." |
| 20 | Click `Clear` on the chat panel | Transcript empties; input returns to ready state. |
| 21 | Click `Close` | App returns to the open form. |

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
Clear chat: PASS / N/A
Close: PASS
Fixture cleanup: PASS
Final git status: ...
```

If any step fails, keep the app and logs around until the failure is
understood. A hang on `Opening...` usually means the frontend IPC bridge
did not reach Rust, so check Tauri capabilities and CSP first.
