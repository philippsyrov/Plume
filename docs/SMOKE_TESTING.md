# Smoke Testing

This checklist proves the packaged desktop app works through the same
visible UI a human or computer-use agent drives. Use it after UI,
IPC, Tauri config, CSP, capability, bundle, or safety-policy changes.

## What It Catches

`npm run tauri dev` is useful for fast iteration, but it runs a raw
debug binary. The smoke path builds and launches a real macOS
`Plume Smoke.app`, so it catches production-bundle problems that dev mode can
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

Use only a managed worktree or `/private/tmp` fixture. Never open a
Desktop-root project with this ad-hoc smoke build.

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

- Prepares the pinned MLX-LM runtime and thin arm64 Apple helper. A cold
  project-local uv cache needs network access for this packaging step.
- Builds Tauri with `CARGO_NET_OFFLINE=true` after resources are ready.
- Produces `src-tauri/target/debug/bundle/macos/Plume Smoke.app`.
- Quits any previous instance of that exact bundle.
- Launches `Plume Smoke.app`.
- macOS / computer-use can target `Plume Smoke` or bundle id
  `dev.plume.smoke`.
- The app bundle contains runtimes and the third-party notice, not model
  weights. Qwen remains an explicit in-app download to Application Support.

Isolation boundary:

- The smoke identity isolates its `.app` path, LaunchServices identity,
  localStorage, and Tauri app-data directory. It does not read, delete,
  reset, or modify real Plume app data.
- This state isolation does not stabilize TCC permission persistence.
  The debug bundle is ad-hoc signed, so macOS may ask again after a
  rebuild. Stable reviewed development signing
  (Apple Development or equivalent) remains a roadmap requirement for this harness.
- Full Disk Access and privacy-setting automation are forbidden. Do not
  request, click, script, or reset privacy settings for smoke testing.

### Library partial-failure fixture (step 60)

Use a dedicated empty disposable project so no existing memory file needs to
be moved or restored. In one terminal, run this block and keep the terminal
open so the variable remains available for cleanup:

```bash
LIBRARY_SMOKE_DIR="$(mktemp -d /private/tmp/plume-library-smoke.XXXXXX)"
mkdir -p "$LIBRARY_SMOKE_DIR/.plume/memory/topics"
printf '# Healthy topic\n\nThis normal topic source should remain readable.\n' > "$LIBRARY_SMOKE_DIR/.plume/memory/topics/healthy.md"
ln -s /etc/hosts "$LIBRARY_SMOKE_DIR/.plume/memory/entries.jsonl"
printf 'Open this disposable project in Plume: %s\n' "$LIBRARY_SMOKE_DIR"
```

After confirming the independent failure in step 60, remove only the planted
symlink, click `Retry project memory`, then close the project and delete the
dedicated fixture:

```bash
rm "$LIBRARY_SMOKE_DIR/.plume/memory/entries.jsonl"
# Click Retry project memory in Plume, then Close after it recovers.
rm -rf "$LIBRARY_SMOKE_DIR"
```

Never run this setup against an existing project. A symlinked individual
`topics/*.md` is skipped rather than failing the topic source, so the refused
`entries.jsonl` symlink plus a normal topic file is the reproducible way to
exercise independent source failure.

## Visual Checklist

Drive the app through visible clicks/keyboard, not hidden IPC.

| Step | Action | Expected |
| --- | --- | --- |
| 1 | Open a project fixture in a managed worktree or `/private/tmp` | Project opens. If already trusted, status strip shows `trusted`, git branch, dirty count, and `npm`. |
| 2 | Trust prompt, when shown | Click `Trust this project`; status strip replaces the trust panel. |
| 3 | Open `docs/BOOTSTRAP.md` | CodeMirror shows text with line numbers and monospace code font. |
| 4 | Open `src-tauri/icons/icon.png` | Binary placeholder appears; bytes are not rendered as text. |
| 5 | Open `.env.smoke` | Red blocked message appears: `.env.smoke is blocked by the secret-filename policy`. |
| 6 | Open `package-lock.json` | JSON renders in CodeMirror and scrolls. |
| 7 | Provider panel, when Ollama or LM Studio is running | Each model row carries a `Select` button. Offline/`not configured` providers render their rows with `Select` disabled. |
| 8 | Click `Select` on a model row | Center "Selected model" banner shows `<Provider> · <model id>` and a Clear button; the picked row gains a `✓ selected` badge. |
| 9 | If Ollama: expand a model first, then click `Select` | Banner additionally renders the fit verdict badge captured at click time. |
| 10 | With no model selected, look at the Chat panel | Prompt input is disabled with placeholder "Pick a model on the left to enable chat."; status reads "No model selected." |
| 11 | With a selected provider that has no chat adapter yet (e.g. LM Studio) | Chat input is disabled with placeholder pointing at the adapter limit; status reads "Selected provider has no chat adapter yet." |
| 12 | With an Ollama model selected and the daemon up, or a Plume-managed MLX model running and selected, type a short prompt and click `Send` | Textarea and Send bar stay visible under the attach/context rows. Send button is replaced by a `Stop` button; an in-progress assistant entry with a blinking cursor appears in the transcript and gains tokens as they stream in. On completion the cursor disappears and the entry shows `served by <model>` + duration, plus a D9 stats footer line when provider metrics are available. |
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
| 24 | Project-instructions indicator before first send: open a project with `AGENTS.md` at root. | A plain **Project instructions** summary appears. Its accessible status says the instructions are ready for the next send; the raw filename and byte/redaction facts are not exposed in the summary. |
| 25 | Open **Project instructions** before sending. | The Details disclosure shows **Next send**, `AGENTS.md`, exact bytes/redactions, and **Ready**. Close the disclosure and confirm the normal composer remains uncluttered. |
| 26 | Send any project message with `AGENTS.md` present, then reopen **Project instructions**. | **Last send** says **Included** and **Next send** remains independently backed by the current preview. Ask a project-specific question and confirm the reply uses the instructions. |
| 27 | Temporarily rename `AGENTS.md` without reopening the project, then wait for context preview and reopen **Project instructions**. | The summary becomes visibly unavailable; Details says Plume could not read the current project instructions. The last-send fact remains separate. Restore the filename or reopen the project. |
| 28 | Attach a small text file such as `docs/BOOTSTRAP.md`. | The context shelf shows one readable file item. Open its **Details** disclosure to confirm the exact path, bytes, and preview readiness. Project instructions remain a separate summary. |
| 29 | Select lines 5–10 of `docs/BOOTSTRAP.md`, then choose **Use selection in chat**. | The shelf shows the readable file/range label. Details preserves the exact `docs/BOOTSTRAP.md:5–10` provenance and bytes; the summary does not lead with raw manifest data. |
| 30 | D12 context preview blocked attachment: temporarily rename `.env.smoke` to live inside the project (e.g. `mv .env.smoke .env`), refresh navigator, try to attach via the chat panel | Attach button stays disabled (D8 already enforces this at the inspector level). For a direct test of the preview's blocked-path: call `chat.context` from devtools with `{ attachment: { kind: 'projectFile', relPath: '.env' } }` while `.env` exists — response shows `attachment.status === 'blocked'`, `reason === 'blocked'`, `message` mentions the secret-filename policy. Restore the fixture name afterwards. |
| 31 | D14 reachability preflight: with an Ollama model selected AND `ollama serve` NOT running, look at the chat status row | Status reads `Ollama not reachable — start the daemon and click Recheck to send.`; placeholder reads `Type your message — start Ollama and click Recheck to send.`; Send button is disabled; a `Recheck` button (warn-coloured) appears next to the status. Typing into the textarea is still allowed so the user can compose while starting the daemon. |
| 32 | D14 recheck round-trip: start `ollama serve` in another shell, then click `Recheck` in the chat panel | Recheck button STAYS visible while the new probe is in flight — the label flips to `Rechecking…` and Send remains disabled, the status reads `Checking Ollama reachability…`. Within ~1 s the status returns to `Ready · Ollama · <model>`, the Recheck button disappears, and Send enables. There should be no intermediate frame where Recheck briefly vanishes or Send briefly enables. |
| 33 | D14 chip restore on synchronous reject: stop `ollama serve` again (so the daemon is down), attach `docs/BOOTSTRAP.md`, type any prompt, click `Send` | The transcript shows an error row (`could not reach ollama at 127.0.0.1:11434…`), AND the attachment chip reappears below the attach bar with the same `docs/BOOTSTRAP.md · <bytes>` content (D14: rejected sends restore the chip so the user doesn't re-attach by hand). |
| 34 | D14 chip stays consumed on successful send: with Ollama running, attach `docs/BOOTSTRAP.md`, send a prompt referencing the file | The chip clears from the form bar on Send (one-shot per accepted send); the user turn carries the inline `¶ docs/BOOTSTRAP.md` chip; the next send starts with no chip attached. |
| 35 | D14 copy button on completed reply: send a prompt that produces a multi-line reply; hover the assistant turn | A subtle `Copy` button appears at the top-right of the entry on hover (and on focus-within for keyboard users). Click it — label flips to `Copied!` for ~2 s, then back to `Copy`. Paste somewhere else to confirm the full reply text was copied. Streaming and cancelled turns deliberately don't show a Copy button. |
| 36 | D15 mode toggle visible in chat header: with a project trusted and a model selected, look at the chat header next to the Clear button | A two-button segmented control labelled `Chat | Propose diff` is visible. `Chat` is the default active option (ink-filled). Clicking `Propose diff` flips the filled state to the second option. The control is disabled while a stream is in flight. |
| 37 | Propose-diff round-trip: switch the response action to **Propose diff**, then request a small change in a disposable project file. | The user turn records Propose diff and the assistant reply renders as a coloured diff panel rather than plain text. Validation runs before Apply becomes available. The Copy action still copies the complete reply. |
| 38 | D15 propose-diff prose fallback: in `Propose diff` mode, send a prompt the model can't honestly turn into a diff (e.g. "what is the capital of France?") | The reply renders as plain text (not as a diff panel) and a warn-coloured `No diff fence detected — model returned prose. Try again or rephrase the request.` hint appears below the entry. The Apply button is not shown for this turn (no diff to apply). |
| 39 | D15 mode persists across follow-ups: in `Propose diff` mode, send a second prompt referencing the first | The new user turn carries the `¶ propose diff` badge; the reply renders as a diff (or shows the prose fallback hint). Flipping the toggle back to `Chat` and sending again produces a normal text reply without the badge — confirming the toggle changes the NEXT send, not previous ones. |
| 40 | Valid-diff validation: re-run step 37 and watch the area below the rendered diff. | `validating diff…` becomes `valid diff · 1 file · M hunks`. **Apply** enables only after validation passes. Do not apply this test diff unless you are using the disposable file from the Apply/Revert steps below. |
| 41 | D16 invalid-diff validation pill via devtools: open the inspector's devtools (right-click → Inspect) and run `await window.__TAURI__.core.invoke('patch_validate', { req: { ipcVersion: 1, payload: { diff: '--- a/../etc/passwd\n+++ b/../etc/passwd\n@@ -1,1 +1,1 @@\n-a\n+A\n' } } })` | Resolves with `{ ok: false, errors: [{ kind: 'pathEscape', message: "path contains '..' component: ../etc/passwd", ... }] }`. Confirms the path-safety guard rejects diffs that escape the project root without ever calling a model. |
| 42 | D16 validation IPC failure pill: in the same devtools, run `await window.__TAURI__.core.invoke('patch_validate', { req: { ipcVersion: 99, payload: { diff: '--- a/x\n+++ b/x\n@@ -1,1 +1,1 @@\n' } } })` | Rejects with `kind: 'Version'`, confirming that envelope-shape errors surface as typed IPC errors. In the chat panel proper, this branch is what would render the `validation unavailable: IPC version mismatch…` pencil pill if the IPC envelope itself ever drifted. |
| 43 | D27 local model inventory empty state: look at the provider panel below the registered-provider rows. With `plume-models/` empty (the default for a fresh checkout), confirm the `Local models` heading is visible and the section reads `No local model files yet.` in pencil italic. Click `Refresh` — the empty state is unchanged. | A `Local models` section renders below the provider list; the empty-state copy is shown verbatim and survives a Refresh. |
| 44 | D27 local model inventory file pickup (optional, no real download): run `printf 'gguf' > plume-models/smoke.gguf` in another shell, click `Refresh` on the provider panel, then run `rm plume-models/smoke.gguf` and click `Refresh` again. `plume-models/` is gitignored, so neither command dirties the working tree. | After the first Refresh the `Local models` section lists a `smoke.gguf` row with a `GGUF` badge and `4 B` size. After deletion + second Refresh the row disappears and the empty-state copy returns. |
| 45 | D30 resize handles + side-panel toggles: hover the 8 px gutter between the left panel and the agent center — the cursor flips to `col-resize` and the stripe darkens to pencil. Drag it left and right; do the same with the gutter between the center and the right inspector. Then click the `‹` chevron in the status strip (left of `Close`) and the `›` chevron next to it. | Each drag visibly resizes the corresponding side; the agent center column never collapses below ~280 px no matter how far the user drags. Each chevron click collapses or restores its panel without flashing the layout, and the chevron glyph flips direction to point toward the edge the panel sits behind. |
| 46 | D30 keyboard shortcuts + persistence on reload: press `Cmd+Shift+[` and `Cmd+Shift+]` to toggle each side from the keyboard. Resize one panel to a non-default width, hide the other, then quit Plume (`Cmd+Q`) and relaunch via `./scripts/smoke-app.sh`. Reopen the same project. | Keyboard shortcuts toggle the same state the chevron buttons do (no focus on a text input required — the listener is window-level). After relaunch the project remembers both panel widths and visibility flags via `localStorage['plume:workspace-layout-v1']`. Devtools → Application → Local Storage shows the persisted JSON shape if you want to confirm. |
| 47 | D31 apply happy path: first create a small test file with `printf 'one\ntwo\nthree\n' > plume-apply-smoke.txt` in another shell (the filename is gitignore-friendly). Switch the chat mode toggle to `Propose diff`. Send a prompt like "in `plume-apply-smoke.txt`, change `two` to `TWO`". When the green `valid diff · 1 file · 1 hunk` pill appears under the rendered diff, click `Apply`. | Apply button label flips to `Applying…` (disabled), then to `Applied` (terminal-disabled). The pill flips to `applied · 1 file · checkpoint <8-char-hex>…` in `--good`. Hover the pill to see the full checkpoint id. In a shell, `cat plume-apply-smoke.txt` shows the post-image (`one\nTWO\nthree\n`). `ls plume-apply-smoke.txt .plume/checkpoints/` lists the patched file and one checkpoint directory containing `manifest.json` plus `files/plume-apply-smoke.txt` (the pre-image copy). Clean up afterwards: `rm plume-apply-smoke.txt && rm -rf .plume/checkpoints/`. |
| 48 | D31 apply pre-image drift rejection: recreate the same file via `printf 'one\ntwo\nthree\n' > plume-apply-smoke.txt`. In `Propose diff` mode, send the same "change `two` to `TWO`" prompt to get a green-validated diff. BEFORE clicking Apply, edit the file from a shell: `printf 'one\nNOPE\nthree\n' > plume-apply-smoke.txt`. Now click `Apply`. | Apply button transitions to `Applying…` then back to `Apply` (not `Applied`). The pill flips to `apply failed (pre-image drift): <message>` in `--bad`, naming the mismatched hunk. The file on disk still reads `one\nNOPE\nthree\n` — Plume detected the drift and wrote nothing. No new checkpoint directory was created (`ls .plume/checkpoints/` is empty or only carries the previous successful run's id). Clean up: `rm plume-apply-smoke.txt && rm -rf .plume/checkpoints/`. |
| 49 | D32 inner-panel chip toggles: at the top of the left column, find the chip strip with `Files`, `Providers`, `Local models`. Click `Providers` once. Click `Local models` once. Then click `Files`. Now look at the right column's chip strip and click `Inspector`. | First click on each chip flips it from filled (visible) to outlined (hidden), and the corresponding panel disappears from the column. After hiding `Files`, the left column shows ONLY the chip strip plus an `EmptyColumn` placeholder reading `No navigation panels visible. Tap a pill above to bring one back, or hide the column entirely from the status strip.` Hiding `Inspector` does the same for the right column. The outer column-level chevron toggles in the status strip still work and hide the entire column. |
| 50 | D32 inner-panel persistence + re-show: with all left-column panels hidden and `Inspector` hidden, quit Plume (`Cmd+Q`) and relaunch via `./scripts/smoke-app.sh`. Reopen the same project. Confirm devtools → Application → Local Storage shows `plume:inner-panels-v1` with all four booleans set to `false`. Then click each pill once to restore visibility. | After relaunch, both columns still show only the chip strip + empty-column placeholder — the hidden state persisted independently from the D30 outer-column key. Each pill click restores its panel inline (no flash, no refetch loop on the provider inventory — `useProviderInventory` is called once at the trusted-view level and both `Providers` and `Local models` re-mount against the cached state). |
| 51 | D33 revert happy path: recreate `printf 'one\ntwo\nthree\n' > plume-apply-smoke.txt`. In `Propose diff` mode, send the "change `two` to `TWO`" prompt; when the green `valid diff` pill appears, click `Apply`. After the pill flips to `applied · 1 file · checkpoint …`, click the `Revert` button that now appears next to the (disabled) `Applied` button. | Revert button label flips to `Reverting…` (disabled), then to `Reverted` (terminal-disabled). The pill flips to `reverted · 1 file restored` in `--good`. In a shell, `cat plume-apply-smoke.txt` shows the pre-image again (`one\ntwo\nthree\n`). The checkpoint directory is preserved under `.plume/checkpoints/` (revert does not delete it — re-revert is intentionally not supported). Clean up: `rm plume-apply-smoke.txt && rm -rf .plume/checkpoints/`. |
| 52 | D33 revert drift rejection: recreate `printf 'one\ntwo\nthree\n' > plume-apply-smoke.txt`. Apply the same `two → TWO` diff to get a green `Applied` state. BEFORE clicking `Revert`, edit the file from a shell: `printf 'a\nb\nc\n' > plume-apply-smoke.txt`. Now click `Revert`. | Revert button transitions to `Reverting…` then back to `Revert` (not `Reverted`). The pill flips to `revert failed (post-apply drift): drift: plume-apply-smoke.txt content differs from the post-apply state …` in `--bad`. The file on disk still reads `a\nb\nc\n` — revert detected the drift and wrote nothing. Clean up: `rm plume-apply-smoke.txt && rm -rf .plume/checkpoints/`. |
| 53 | D33 rename apply: create a fresh file with `printf 'sample\nbody\n' > plume-rename-smoke.txt`. In `Propose diff` mode, send a prompt like "rename `plume-rename-smoke.txt` to `plume-rename-smoke-2.txt` and capitalise both words". When the green `valid diff` pill appears with a single rename touch (the diff body should show `--- a/plume-rename-smoke.txt\n+++ b/plume-rename-smoke-2.txt` headers plus a hunk), click `Apply`. | The old filename is gone; `cat plume-rename-smoke-2.txt` shows the post-image (`SAMPLE\nBODY\n` or whatever the model produced). Checkpoint directory contains `files/plume-rename-smoke.txt` (the pre-image at the OLD path) AND `post/plume-rename-smoke-2.txt` (the post-image at the NEW path). Clicking `Revert` after a successful rename apply restores both the old name AND the original content. Clean up: `rm plume-rename-smoke-2.txt plume-rename-smoke.txt 2>/dev/null; rm -rf .plume/checkpoints/`. |
| 54 | Click `Clear` on the chat panel | Transcript empties; input returns to ready state. Preview stays visible (clearing transcript doesn't clear chip). |
| 55 | Click `Close` | App returns to the open form. |
| 56 | With no project open, open **Settings → Library**. Under **About you**, save `Prefers plain English smoke answers`, edit it once, then leave Settings. | About you works without project trust. **This project** explains that a trusted project is required. The saved entry is app-private, redacted/capped by the backend, and survives closing Settings. |
| 57 | Open **Library** from the sidebar with no project open. Select **About you**, search for mixed-case `PLAIN`, clear the query, and open the saved entry's **Details**. | About you is available and search is case-insensitive inside that selected source only. **This project** and **Topics** are unavailable rather than shown as empty user memory. Details shows provenance fields; no link, retrieval, or automatic-context claim appears. |
| 58 | From the About you row, click **Use in chat**. Return to Library and drag the same row into the visible chat drop target. | The local chat shelf receives one exact User memory item. The duplicate drag emphasizes the existing row rather than adding another. Merely browsing, searching, or saving the entry never adds it automatically. |
| 59 | Open and trust a normal project. Open Library and compare **About you**, **This project**, and **Topics**. | The same app-private About you entry remains visible; project memory/topics belong only to this trusted project. The source tree and search copy make the selected boundary explicit. |
| 60 | Follow **Library partial-failure fixture** above: open and trust the generated project while its `entries.jsonl` symlink is planted, then remove only that symlink and click **Retry project memory**. | About you and the normal `healthy.md` topic stay usable while This project reports its own refused-symlink error. Retry recovers project memory to the empty state without disturbing either healthy source. |
| 61 | Open a normal project A in Library, note a distinctive topic or memory, then close it and open a different project B. Inspect Library and **Settings → Library** in B. | No project memory/topic from A appears in B; About you remains because it is app-private. Library browsing stays read-only and Settings owns mutations. The stricter stale in-flight/unmount race is pinned in `src/features/library/useLibraryData.test.tsx`; this manual step does not claim to force timing without a delay hook. |
| 62 | In a project with one memory linked to `topics/context-smoke.md`, select that topic and inspect **Connections**. Also inspect a memory with a definitely missing link; if the topic list is capped, inspect a canonical link beyond the returned prefix. | Topic detail renders capped Markdown and only exact stored backlinks. Missing links say `missing topic`; capped-out canonical refs plainly say Plume could not verify them because only part of the topic list loaded. Connections says it organizes information and does not choose chat context. |
| 63 | Select **This project**, search mixed-case text that matches one project memory, then switch to **About you** and repeat. | Search stays inside the selected visible source. It does not silently aggregate About you, another project, topics, or hidden rows. Clearing restores that source's normal index. |
| 64 | On one project memory and one eligible `topics/*.md` file, click **Use in chat**; return and drag each source into chat again. | Each click adds only its exact opaque ref to project chat. Duplicate drags emphasize existing rows. Core `INDEX.md` / `USER.md` / `SOUL.md` files have no explicit action, and links/backlinks add nothing by themselves. |
| 65 | Open a project file, select a few lines, and click **Use selection in chat**. | Project chat opens with a visible Context shelf item naming the exact `path:start–end`. Its preview settles from `checking…` to a byte count. The item remains after changing files. |
| 66 | Remove one project-only shelf item, quit Plume, relaunch, reopen the same project session, then switch to another project and a local chat. | The remaining ordered project shelf returns only with its owning project session. The removed item stays removed. Other projects receive no leaked refs; local chat retains only its own About you/Browser refs and rejects project file, project-memory, or topic refs. |
| 67 | Add `topics/context-smoke.md`, use it in chat, delete the file outside Plume, and return to chat. | The topic shelf item becomes visibly blocked with a useful reason. Other ready items remain visible. Send cannot start until the stale item is removed or restored; Plume never substitutes a neighboring topic or linked memory. |
| 68 | With a reachable local model, send once with exact file selection, About you entry, project memory, and topic refs. | The shelf stays sticky. The accepted user turn gains immutable chips for the exact backend-accepted file range, user-memory id/preview, project-memory id/preview, and topic name. Ambient project memory excludes only the explicitly selected project entry; About you remains explicit-only. |
| 69 | Use **Continue in new chat** or **Rewind into new chat** from that session. | Historical user-turn manifest chips remain on copied turns, but the child chat starts with an empty current shelf. User/project source ownership and manifest kinds survive relaunch without broadening scope. |

### Chat sessions (D63B) — no model required

Persistence UI works against mock/disabled chat state; do not start or
download a local model for these steps.

| Step | Action | Expected |
| --- | --- | --- |
| S1 | Launch without a project; click `New chat` twice | Two `New chat` rows under **Chats**, newest first; the newest is selected. |
| S2 | Row menu `…` → `Rename`, enter `First smoke chat` | Plume-styled dialog (no browser prompt). Row title updates only after Save; Escape/Close leaves it unchanged. |
| S3 | Quit (`Cmd+Q`), relaunch, stay projectless | Both rows return; the most recently updated chat is selected and its transcript (if any) restored. |
| S4 | Open + trust a project; click the `+` on the project row | A project chat row appears under the project — and NOT under **Chats**. Local rows stay under **Chats** only. |
| S5 | Row menu `…` → `Archive` on a local chat | Row leaves the list; an `Archived chats` action appears at the bottom of the section. Open it → modal lists the chat; `Unarchive` restores it at its historical position. |
| S6 | Row menu `…` → `Delete` | Explicit `Delete permanently` confirmation dialog. After confirming and relaunching, the chat is gone. |
| S7 | Without a running model, type a prompt in a chat (send is disabled) then click between rows | Switching is instant — no stream is active, so no block. The composer stays per-session (draft does not leak between rows). |
| S8 | Close the project (`Close`) | Back on the no-project surface, the **Chats** list shows the same local rows; project rows are gone with the project. |
| S9 | (D65 — needs an accepted send: a running model, or a send that errors AFTER the user turn appears) On a fresh `New chat`, send a message with messy whitespace (e.g. `  fix the   login` ⏎ `bug  `) | The row title becomes `fix the login bug` (trimmed, runs collapsed) once the turn is accepted; long messages cap at ~60 chars with `…`. Relaunch: the derived title persists. |
| S10 | (D65) Rename a chat manually, then keep sending messages; also try renaming a chat to exactly `New chat` | The manual title never changes as messages arrive. The rename dialog refuses the literal `New chat` with a visible message — it is reserved for untitled chats, which is what keeps manual titles safe across relaunches. |
| S11 | (D66) Click `Search chats` (or press Cmd+K); type part of a chat title, then part of an old message | Compact overlay opens with the input focused. Title matches list first; transcript matches show a highlighted snippet. In a project window, local and project results sit in separate sections. Enter or click opens the chat and closes the overlay; Escape closes it. |
| S12 | (D66) Search for text that only exists in an archived chat; also type `NEAR(` or `docks OR treasure` | The archived chat appears with an `archived` badge and opens on selection. Operator-looking text is searched literally — no error, no surprise semantics. |

### Integrated task Browser — no model required

Browser is owned by the exact persisted chat that opened it. The smoke below
proves split/expanded layout, native WKWebView navigation, per-chat restoration,
and explicit evidence handoff without granting pages Plume IPC.

Start a disposable localhost fixture in `/private/tmp`; never serve a real
project or home directory:

```bash
BROWSER_SMOKE_DIR="$(mktemp -d /private/tmp/plume-browser-smoke.XXXXXX)"
printf '<!doctype html><title>Plume smoke</title><h1>Local browser smoke</h1><p id="capture">Selected browser evidence</p><a href="/next.html">Next page</a>' > "$BROWSER_SMOKE_DIR/index.html"
printf '<!doctype html><title>Next</title><h1>History works</h1>' > "$BROWSER_SMOKE_DIR/next.html"
python3 -m http.server 57880 --bind 127.0.0.1 --directory "$BROWSER_SMOKE_DIR"
```

Keep that terminal open while running the steps, then stop it with Ctrl+C and
delete only the disposable fixture: `rm -rf "$BROWSER_SMOKE_DIR"`.

| Step | Action | Expected |
| --- | --- | --- |
| B1 | Launch projectless; open Workspace views → Browser | Plume creates/selects a local chat, then opens Browser beside that same chat. Tabs, address, Back, Forward, Reload, and evidence controls are visible. |
| B2 | Enter `https://example.com` | The page renders inside the Browser pane. Popups, downloads, page IPC, and devtools remain denied. Do not claim a stronger macOS autofill/extension boundary than `SAFETY.md` documents. |
| B3 | Click Expand; choose **Show chat**, then **Hide chat**; return to split | Browser first fills the canvas to the bottom. Show chat opens the compact raised composer in a reserved bottom zone; Hide chat restores the full page; split restores the two-pane layout. |
| B4 | Add a tab, navigate, switch tabs, then relaunch the app and reopen this chat's Browser | Tab order, admitted history, active tab, and layout restore for this chat only; cookies remain in the WKWebView profile rather than the session database. |
| B5 | Open another local chat's Browser | It starts with its own Browser workspace; tabs/history from B4 do not appear. |
| B6 | Open and trust the disposable project fixture, create/open a project chat, then Browser | Browser is owned by that project chat. Enter `127.0.0.1:57880`; exact-origin confirmation appears. Local chat does not offer loopback approval. |
| B7 | Confirm the local origin, click `Next page`, then Back / Forward / Reload | The fixture stays inside the integrated page pane and the visible controls track its history. |
| B8 | Select `Selected browser evidence`; open **Attach** and choose **Selected text** | One emphasized Web context chip appears in the beside chat. Context preview reports it ready; no raw captured body is rendered in Browser chrome. |
| B9 | Return to Browser; choose **Attach → Readable page text**, then send a project message with a running model | A second Web chip is added. The accepted user turn preserves exact URL/title/capture-kind/bytes/redaction/truncation provenance. Reloading or changing the page afterward does not change the historical manifest. |
| B10 | Return to Browser and choose **Attach → Visible screenshot** | Plume captures the visible WKWebView viewport, returns to project chat, and adds one Image chip with exact URL/title/dimensions/bytes provenance. With no selected model or a text-only model, preview stays visibly blocked and the image is not sent. |
| B11 | With an exact Ollama model whose fresh `/api/show` response includes `vision`, send with the screenshot; then repeat with a text-only model | The vision model receives the PNG only on the final user message and the accepted turn persists the exact screenshot manifest. The text-only attempt fails before stream registration and keeps the shelf intact. MLX remains text-only. |
| B12 | Start a capture while the page is loading or navigate/project-switch before it finishes | Capture fails visibly with short retry copy and no new chip. No hidden navigation or agent control appears. |
| B13 | Begin typing a different address, wait at least one second without pressing Go, then switch split/expanded or relaunch and return to this chat | The unfinished address draft remains with this chat and does not overwrite another chat's address. |
| B14 | Open **Attach**, then click directly inside the native page | The menu closes as focus crosses into the child WebView. Keyboard/assistive navigation still exposes the Attach button and its three plainly labelled choices. |
| B15 | Use a test fixture whose persisted Browser row is corrupt or whose saved URL was privacy-reduced | Transcript remains intact. Corrupt state shows a reset notice; a reduced URL stays closed until **Reopen page** is clicked, and loopback still requires the trusted-project exact-origin confirmation. |
| B16 | With Browser visible, enter text or scroll to a recognizable live page state. Open Workspace views, Settings, Help, Search, and a chat action dialog; close each one, then resize and move the Plume window between checks. | Each overlay waits for acknowledged native-page suspension, then appears with no page pixels above it. Closing the overlay reveals the same live page state without a reload. During movement/resizing no stale page rectangle remains outside the window. |
| B17 | Persist a wide split Browser layout, quit Plume, relaunch at a narrower window width, and reopen that chat's Browser. | The split descriptor is normalized to the narrower measured canvas: chat and Browser remain usable, the divider stays in bounds, and the corrected width persists for the next relaunch. |

Rejected and never-resolving suspension paths remain deterministic unit-test
evidence. Packaged smoke verifies the normal native-layer ordering above; it
does not ship or claim a production fault-injection path.

### Bounded research note — packaged model required

Run this exact-head matrix before changing `research.bounded-notes` from
`partial` to `shipped`. Use only text evidence captured manually in the owning
Browser; do not treat model loopback traffic as evidence-network access.

| Step | Action | Expected |
| --- | --- | --- |
| R1 | Attach 2–3 Browser selected/readable text records to one local chat; keep an ineligible shelf item present if available. Open **Create → Research note**. | Only eligible exact Browser text is counted. Copy states Markdown, chosen model, 10-source, 13-step, and 26-call ceilings. No search/fetch claim appears. |
| R2 | Run with the installed fixed Qwen handle. | Visible progress advances, chat/context mutation stays disabled, and completion yields one Preview/Sources/Details card. Details stay at or below 13 logical turns and 26 provider calls. |
| R3 | Inspect preview and sources. | Footnotes are visible inert text; no link/image/HTML activates. Every citation resolves to the exact immutable source list. Provenance copy does not claim truth or relevance. |
| R4 | Trigger or retain a malformed-frame/context-overflow fixture through the packaged test build. | One recovery is visible for that logical turn; malformed re-ask and repack share the allowance. The run then completes honestly or fails visibly within ceilings. |
| R5 | Run again and choose **Stop** during generation. | Stop stays reachable, terminal becomes stopped, and no stale artifact/event repaints a later owner or run. |
| R6 | Export the exact artifact; cancel once, then save. | Cancel is quiet. The native panel proposes `research-note.md`; saved Markdown matches the loaded version and Plume reports only the file name, never its path. |
| R7 | On a host reporting Apple On-Device available, repeat R2–R3 with Apple. | Apple uses the same artifact/event/citation contract without a Qwen fallback. On macOS 26.0–26.3 the conservative 4,096-token estimation path is expected; on unavailable hosts record N/A plus the exact host reason. |
| R8 | Produce a citation-invalid draft fixture or natural small-model result. | `Draft — citations need review` is a normal visible terminal with Preview/Sources/Details and export still reviewable; it is not mislabeled verified. |

Record exact app source SHA, package SHA if applicable, host/OS, model identity,
source count, terminal status, logical turns/provider calls, export outcome, and
whether any non-model-transport network I/O occurred.

Recorded 2026-07-19 on Apple Silicon, macOS 27.0 beta build 26A5378n:

- Packaged `Plume Smoke.app` at implementation head `2807c3f` used one exact
  129-byte Example Domain Browser-text capture with Apple On-Device. A natural
  malformed framing path visibly consumed the one retry and failed closed; a
  tighter retry produced an ordinary `Draft — citations need review` in 4
  logical turns / 8 model calls. Native export cancel was quiet, save reported
  only `plume-research-smoke.md`, the exported bytes had SHA-256
  `dc5d7bb18df995f4395eb445b894aeb83d272ea0d12695e66ddc9c85f1621a23`,
  and quit/relaunch restored the exact artifact and source.
- The same isolated app exposed a real packaged defect: fixed-Qwen download
  stayed at 0% because the async command constructed a blocking reqwest client
  on Tokio and panicked before returning an operation id. Implementation head
  `b32ce2c` moved only the start handshake to the blocking pool. The rebuilt app
  then advanced immediately, downloaded and hash-verified the pinned
  `b3252a2f97102b1fb1571fec2c9b27219a8536be` revision (880,170,581 installed
  bytes), started the bundled MLX-LM runtime without Ollama or user Python, and
  produced a Qwen note in 4 logical turns / 4 model calls / 3,481 ms. Qwen
  omitted the citation marker, so Plume correctly staged it for review.
- Stop during an Apple model turn reached the stopped terminal at `b32ce2c`.
  That package exposed a feedback bug: the completed last step masked
  `Research stopped.`. Final implementation head
  `5c88b2f3658c25d5acec49e845c93d3272374fd8` fixes and tests that terminal
  projection. Its rebuilt executable SHA-256 is
  `ca8368f840e0009cd457e1053ab2f046cb9fcf3045a86bc7739328b8ae52f30f`;
  it restored the Qwen artifact and produced a citation-verified Apple note.
- Browser navigation/capture was human-driven before each run. The harness did
  zero non-model-transport network I/O; the only network transfer above was the
  explicit fixed-model download. Context-overflow repacking, malformed-response
  fixtures beyond the natural results, and stale-owner fencing remain automated
  test evidence rather than claimed production fault-injection UI.

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
Reachability preflight shows "not reachable" when Ollama is down: PASS / N/A
Recheck button flips status to Ready after daemon starts: PASS / N/A
Chip restores after a synchronous send rejection: PASS / N/A
Chip clears on accepted send; assistant turn carries inline chip: PASS / N/A
Copy button on completed assistant reply copies full text: PASS / N/A
Mode toggle (Chat | Propose diff) visible and disables on stream: PASS / N/A
Propose-diff reply renders a coloured diff; Apply enables only after validation: PASS / N/A
Propose-diff prose fallback shows "no diff fence detected" hint: PASS / N/A
Mode change applies only to the next send: PASS / N/A
D16 valid-diff pill shows touches + hunks under the rendered diff: PASS / N/A
D16 invalid-diff pill via devtools probe (..-path rejected as pathEscape): PASS / N/A
D16 envelope-mismatch surfaces as typed Version error via devtools probe: PASS / N/A
D27 Local models empty state visible with `No local model files yet.`: PASS / N/A
D27 Local models picks up a dummy smoke.gguf and clears on delete: PASS / N/A
D30 resize handles drag both sides; center never collapses below ~280px: PASS / N/A
D30 chevron toggles + Cmd+Shift+[/] keyboard shortcuts; widths and visibility persist across relaunch: PASS / N/A
D31 patch.apply happy path: Apply button flips to Applied, post-image on disk, .plume/checkpoints/<id>/ created: PASS / N/A
D31 patch.apply pre-image drift rejection: pill flips to apply failed (pre-image drift), file on disk unchanged: PASS / N/A
D32 inner-panel chips hide each panel + show EmptyColumn placeholder when all are off: PASS / N/A
D32 inner-panel visibility persists across relaunch and re-show restores panels inline: PASS / N/A
D33 patch.revert happy path: Revert button flips to Reverted, pre-image restored on disk: PASS / N/A
D33 patch.revert drift rejection: pill flips to revert failed (post-apply drift), file on disk unchanged: PASS / N/A
D33 rename apply: renamed-with-edits writes new path, removes old, Revert restores both: PASS / N/A
Settings Library About you CRUD works projectless and persists app-private state: PASS / N/A
Projectless Library shows About you while This project and Topics stay unavailable: PASS / N/A
Library search stays inside the selected source; Details keeps provenance progressive: PASS / N/A
Library topic detail shows capped Markdown, exact backlinks, and metadata-only Connections copy: PASS / N/A
Library partial-failure fixture leaves About you/topics healthy and Retry recovers project memory: PASS / N/A
Library project A→B switch has no project-data bleed; About you remains app-private: PASS / N/A
Typed shelf shows ordered file/selection, memory, and topic refs with ready/blocked state: PASS / N/A
Typed shelf persists only with its project session and stays sticky after send: PASS / N/A
Accepted user turn shows the exact backend manifest; stale source blocks before streaming: PASS / N/A
Continue/rewind child keeps historical manifests but starts with an empty shelf: PASS / N/A
Library About you click/drag adds one exact local/project user-memory row and never attaches automatically: PASS / N/A
Library project memory/topic click/drag stays project-only; duplicates emphasize rather than duplicate: PASS / N/A
Files inspector drag preserves the current whole-file or exact selected-line provenance: PASS / N/A
Full/unavailable drop stays in the source view and announces the result; ordinary Use in chat still works: PASS / N/A
Reduced-motion mode removes tray and shelf-emphasis animation: PASS / N/A
Help opens from Chat and Project without a network connection: PASS / N/A
Open full Handbook renders the bundled guide inside Plume: PASS / N/A
Local Chat shows and removes explicitly attached Browser/About-you context: PASS / N/A
Settings, Help, and Open Project stay readable in the default warm-paper appearance: PASS / N/A
Appearance starts Light; System follows macOS; explicit Light/Dark persist locally: PASS / N/A
Browser HTML overlays and live window resize never leave the native page above or outside Plume: PASS / N/A
Modal Tab/Shift+Tab stays inside; Escape closes and restores focus: PASS / N/A
Settings hides advanced project tools by default and exposes no developer dry-run panel: PASS / N/A
Trust details stay behind Technical details; no Simple chat label is visible: PASS / N/A
Clear chat: PASS / N/A
Close: PASS
Fixture cleanup: PASS
Final git status: ...
```

If any step fails, keep the app and logs around until the failure is
understood. A hang on `Opening...` usually means the frontend IPC bridge
did not reach Rust, so check Tauri capabilities and CSP first.
