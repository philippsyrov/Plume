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

- Builds with `CARGO_NET_OFFLINE=true`.
- Produces `src-tauri/target/debug/bundle/macos/Plume Smoke.app`.
- Quits any previous instance of that exact bundle.
- Launches `Plume Smoke.app`.
- macOS / computer-use can target `Plume Smoke` or bundle id
  `dev.plume.smoke`.

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

### Knowledge partial-failure fixture (step 60)

Use a dedicated empty disposable project so no existing memory file needs to
be moved or restored. In one terminal, run this block and keep the terminal
open so the variable remains available for cleanup:

```bash
KNOWLEDGE_SMOKE_DIR="$(mktemp -d /private/tmp/plume-knowledge-smoke.XXXXXX)"
mkdir -p "$KNOWLEDGE_SMOKE_DIR/.plume/memory/topics"
printf '# Healthy topic\n\nThis normal topic source should remain readable.\n' > "$KNOWLEDGE_SMOKE_DIR/.plume/memory/topics/healthy.md"
ln -s /etc/hosts "$KNOWLEDGE_SMOKE_DIR/.plume/memory/entries.jsonl"
printf 'Open this disposable project in Plume: %s\n' "$KNOWLEDGE_SMOKE_DIR"
```

After confirming the independent failure in step 60, remove only the planted
symlink, click `Retry memory entries`, then close the project and delete the
dedicated fixture:

```bash
rm "$KNOWLEDGE_SMOKE_DIR/.plume/memory/entries.jsonl"
# Click Retry memory entries in Plume, then Close after it recovers.
rm -rf "$KNOWLEDGE_SMOKE_DIR"
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
| 24 | D11 instructions indicator before first send: a Plume-like project (with `AGENTS.md` at root) shows a `¶ AGENTS.md available` badge in the chat header. Hover for the tooltip. | Badge visible next to the `read-only` badge; tooltip says "will be folded in on your next send". Subtitle mentions AGENTS.md will ride along. |
| 25 | D11 instructions confirmation: send any prompt with `AGENTS.md` present. After the stream completes the badge label flips to `¶ AGENTS.md included` with a "backend confirmed" tooltip. The subtitle also updates to past tense. | Badge label changes after the first accepted send. |
| 26 | D11 instructions effect: ask a project-specific question like "what is rule #1 in this project?" | Reply quotes or references the relevant AGENTS.md content (model-behavior dependent — close enough is a pass). If you temporarily rename `AGENTS.md` (without re-opening the project) and re-ask, the model loses that context AND the badge flips to `¶ AGENTS.md skipped` (warn-colored): the backend reports `instructionsIncluded=false` while `ProjectMeta.hasAgentsMd` is still cached as true. Restore the filename or re-open the project to clear that state — if you re-open with no AGENTS.md on disk, the badge disappears entirely on the next refresh. |
| 27 | D12 context preview baseline: open a project with `AGENTS.md` at root, do NOT attach a file | "Context preview:" row appears between the attach bar and the textarea. A single `¶ AGENTS.md · <bytes>` chip is visible (hover tooltip shows redaction count). With no AGENTS.md the row is hidden entirely. |
| 28 | D12 context preview ready attachment: attach a small text file (e.g. `docs/BOOTSTRAP.md`) | A second `¶ docs/BOOTSTRAP.md · <bytes>` chip appears next to AGENTS.md. Tooltip describes "read-only context." Both chips ride along on the next send. |
| 29 | D12 context preview selection range: select lines 5–10 of `docs/BOOTSTRAP.md` in the inspector, click `Attach selection` | Attachment chip in the preview flips to `¶ docs/BOOTSTRAP.md:5–10 · <bytes>`. Bytes reflect the WHOLE file (the preview reads the full file so the redactor sees lines outside the range). |
| 30 | D12 context preview blocked attachment: temporarily rename `.env.smoke` to live inside the project (e.g. `mv .env.smoke .env`), refresh navigator, try to attach via the chat panel | Attach button stays disabled (D8 already enforces this at the inspector level). For a direct test of the preview's blocked-path: call `chat.context` from devtools with `{ attachment: { kind: 'projectFile', relPath: '.env' } }` while `.env` exists — response shows `attachment.status === 'blocked'`, `reason === 'blocked'`, `message` mentions the secret-filename policy. Restore the fixture name afterwards. |
| 31 | D14 reachability preflight: with an Ollama model selected AND `ollama serve` NOT running, look at the chat status row | Status reads `Ollama not reachable — start the daemon and click Recheck to send.`; placeholder reads `Type your message — start Ollama and click Recheck to send.`; Send button is disabled; a `Recheck` button (warn-coloured) appears next to the status. Typing into the textarea is still allowed so the user can compose while starting the daemon. |
| 32 | D14 recheck round-trip: start `ollama serve` in another shell, then click `Recheck` in the chat panel | Recheck button STAYS visible while the new probe is in flight — the label flips to `Rechecking…` and Send remains disabled, the status reads `Checking Ollama reachability…`. Within ~1 s the status returns to `Ready · Ollama · <model>`, the Recheck button disappears, and Send enables. There should be no intermediate frame where Recheck briefly vanishes or Send briefly enables. |
| 33 | D14 chip restore on synchronous reject: stop `ollama serve` again (so the daemon is down), attach `docs/BOOTSTRAP.md`, type any prompt, click `Send` | The transcript shows an error row (`could not reach ollama at 127.0.0.1:11434…`), AND the attachment chip reappears below the attach bar with the same `docs/BOOTSTRAP.md · <bytes>` content (D14: rejected sends restore the chip so the user doesn't re-attach by hand). |
| 34 | D14 chip stays consumed on successful send: with Ollama running, attach `docs/BOOTSTRAP.md`, send a prompt referencing the file | The chip clears from the form bar on Send (one-shot per accepted send); the user turn carries the inline `¶ docs/BOOTSTRAP.md` chip; the next send starts with no chip attached. |
| 35 | D14 copy button on completed reply: send a prompt that produces a multi-line reply; hover the assistant turn | A subtle `Copy` button appears at the top-right of the entry on hover (and on focus-within for keyboard users). Click it — label flips to `Copied!` for ~2 s, then back to `Copy`. Paste somewhere else to confirm the full reply text was copied. Streaming and cancelled turns deliberately don't show a Copy button. |
| 36 | D15 mode toggle visible in chat header: with a project trusted and a model selected, look at the chat header next to the Clear button | A two-button segmented control labelled `Chat | Propose diff` is visible. `Chat` is the default active option (ink-filled). Clicking `Propose diff` flips the filled state to the second option. The control is disabled while a stream is in flight. |
| 37 | D15 propose-diff round-trip: flip the mode toggle to `Propose diff`, send a prompt like "rename `formatBytes` to `formatSize` in `src/features/chat/ChatPanel.tsx`" | The user turn carries a small inline `¶ propose diff` badge alongside any attachment chip. The assistant reply renders as a coloured diff panel (additions green, deletions red, hunk headers pencil), NOT as plain text. A disabled `Apply` button + italic `preview only — no writes` note appear below the diff. Hover the Apply button — tooltip names the boundary. The D14 Copy button on the assistant entry still works and copies the full reply text (including fence markers). |
| 38 | D15 propose-diff prose fallback: in `Propose diff` mode, send a prompt the model can't honestly turn into a diff (e.g. "what is the capital of France?") | The reply renders as plain text (not as a diff panel) and a warn-coloured `No diff fence detected — model returned prose. Try again or rephrase the request.` hint appears below the entry. The Apply button is not shown for this turn (no diff to apply). |
| 39 | D15 mode persists across follow-ups: in `Propose diff` mode, send a second prompt referencing the first | The new user turn carries the `¶ propose diff` badge; the reply renders as a diff (or shows the prose fallback hint). Flipping the toggle back to `Chat` and sending again produces a normal text reply without the badge — confirming the toggle changes the NEXT send, not previous ones. |
| 40 | D16 valid-diff validation pill: re-run step 37 (a real propose-diff send against a small in-project file) and watch the area between the rendered diff body and the Apply row | While the IPC is in flight a `validating diff…` pencil line appears under the diff. Within ~1 s it flips to `valid diff · 1 file · M hunks` (in `--good`). The Apply button STAYS disabled but its tooltip flips to `Validation passed, but Plume does not apply patches yet…`. The D14 Copy button on the assistant entry still works. |
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
| 56 | Open **Workspace views** → **Knowledge**. | The top bar and main region change to Knowledge, and the drawer closes. |
| 57 | Select an existing topic. | The topic renders capped Markdown and lists only memories carrying that topic's exact canonical ref as backlinks. |
| 58 | Open **All memories**, **Unlinked**, and **Stale links**. If the project has more topic files than the displayed cap, also inspect a memory linked to a canonical topic beyond the returned prefix. | Counts and provenance match each view. A definitively stale or malformed ref is labelled missing and never opens another topic. When topic coverage is partial, the workspace says so; a capped-out canonical ref is labelled not verified and is excluded from Stale links. |
| 59 | Choose **Unlinked**, enter mixed-case text in **Search memories** that also matches a linked entry outside that view, then clear it. | Search covers all loaded memory text with case-insensitive lexical matching, so the linked match can appear while the query is active. Clearing restores the selected Unlinked view. |
| 60 | Follow **Knowledge partial-failure fixture** above: open and trust the generated project while its `entries.jsonl` symlink is planted, then remove only that symlink and click `Retry memory entries`. | The normal `healthy.md` topic stays visible while memory entries report their own refused-symlink error. Retry recovers memory entries to the empty state without disturbing the topic source. |
| 61 | Open a normal project A in Knowledge, note a distinctive topic or memory, then close it and open a different project B. Inspect Knowledge and **Settings** in B. | No topic or memory from A appears in B. Knowledge remains read-only; Settings still owns every mutation. The stricter stale in-flight ordering is automated evidence in `src/features/knowledge/useKnowledgeData.test.tsx`; this packaged smoke does not claim to force that race without a delay mechanism. |

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
Propose-diff reply renders coloured diff with disabled Apply + preview-only note: PASS / N/A
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
Knowledge opens from Workspace views and closes the drawer: PASS / N/A
Knowledge topic shows capped Markdown and exact-ref backlinks only: PASS / N/A
Knowledge All memories, Unlinked, and Stale links show counts and provenance; capped-out canonical refs stay not verified and outside Stale links: PASS / N/A
Knowledge lexical search covers all loaded memories and clears back to the chosen view: PASS / N/A
Knowledge refused-entries fixture leaves topics healthy and Retry recovers entries: PASS / N/A
Knowledge ordinary project A→B switch has no data bleed; Settings owns mutations: PASS / N/A
Clear chat: PASS / N/A
Close: PASS
Fixture cleanup: PASS
Final git status: ...
```

If any step fails, keep the app and logs around until the failure is
understood. A hang on `Opening...` usually means the frontend IPC bridge
did not reach Rust, so check Tauri capabilities and CSP first.
