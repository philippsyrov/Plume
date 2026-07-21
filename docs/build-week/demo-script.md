# Demo Storyboard And Script

Target runtime: **2:45–2:55**. The final video must be public on YouTube, under
three minutes, and include audible narration explaining both the product and
how Codex with the GPT-5.6 family was used.

## Recording setup

- Use the packaged Apple Silicon build, not a dev server.
- Prepare one saved chat with an exact Browser text capture from a reliable
  dinosaur reference page.
- Start with a clean transcript and rehearse the short mouse path.
- Smoke the exact Apple, Qwen Coder, or Qwen2-VL model before recording. Never fake a reply.

## Timed script

**0:00–0:20 — Problem and hook**

> Local AI should spend the laptop on the model, not the app shell. Plume uses
> Tauri instead of Electron, keeps data local by default, and makes every source
> given to the model visible.

Show the clean chat and choose the installed fixed Qwen2-VL model.

**0:20–0:48 — Human-controlled Browser**

Open Plume's Browser, visit the prepared dinosaur page, and attach the visible
screenshot to the chat. Let the screenshot chip settle into the conversation,
then ask Qwen2-VL what it shows. Attach readable page text after the answer.

> This Browser belongs to the saved chat and stays under my control. Plume will
> research only the exact evidence I attach—it does not search or steer the
> page. The screenshot goes to a small local Qwen2-VL model; the text keeps the
> later citations exact.

The attached page is **pinned exact context**. If project memory is mentioned,
describe it separately as visible **bounded ambient context**; do not imply the
two paths have the same authority.

Switch to Qwen Coder 1.5B in the same chat. Plume unloads Qwen2-VL first, so the
two models do not compete for memory.

**0:48–1:25 — Ask normally**

Send `Research what we know about feathered dinosaurs`.

> There is no research mode, selector, or control card. I just ask in the
> conversation. Qwen Coder works through a narrow, bounded workflow over the
> source I chose.

Show the calm progress line and **Stop research** without opening extra UI.

**1:25–1:55 — Answer and source**

Read the ordinary assistant answer, then click its source link to return to the
page in Plume's Browser.

> The result stays in the transcript like any other reply. Source links reopen
> my human-controlled Browser; they do not give the model Browser authority.

**1:55–2:20 — Export only when asked**

Send `Export this as Markdown`, choose a destination in the native macOS save
panel, then show the single filename attachment in the chat.

> Export appears only because I asked for it. Plume adds one clean Markdown
> attachment instead of leaving a toolbar on screen.

**2:20–2:48 — Build Week and Codex**

> During Build Week I used Codex with Sol from the GPT-5.6 family to extend an
> existing editor with explicit context, bounded research artifacts, durable
> transcript references, Browser evidence, and local-runtime hardening.

Show the About/Handbook surface or a simple end card with the repository and
category: **Developer Tools**.

**2:48–2:55 — Close**

> Plume is local AI with less shell, less clutter, and honest control.

## Do not claim

- agent-controlled Browser or computer use;
- broad shell or arbitrary tool execution;
- semantic retrieval or an autonomous multi-step loop;
- support beyond macOS Apple Silicon for this judge build;
- Developer ID signing or notarization unless completed later; or
- that the non-Electron architecture is already a published SSD-wear
  benchmark; numeric comparisons need recorded exact-head evidence.
