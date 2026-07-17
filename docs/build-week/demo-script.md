# Demo Storyboard And Script

Target runtime: **2:45-2:55**. The final video must be public on YouTube, under
three minutes, and include audible narration explaining both the product and
how Codex with the GPT-5.6 family was used.

## Recording setup

- Use the packaged Apple Silicon build, not a dev server.
- Use a tiny disposable project with one short, readable code file.
- Start with a clean chat and keep the mouse path rehearsed.
- Record at a readable resolution; zoom or crop so context-card text is legible.
- If demonstrating a model response, configure and smoke that exact local model
  before recording. Otherwise use the fully verified no-model path and do not
  fake a response.

## Timed script

**0:00-0:18 — Problem and hook**

> AI coding tools use a lot of hidden context. Plume is a local-first workspace
> that makes the files, browser evidence, and memory given to an agent visible,
> inspectable, and removable before anything is sent.

Show Plume opening a disposable project and the trust approval.

**0:18-0:48 — File context**

Open the short code file and choose **Use in chat**.

> I choose this file explicitly. Plume shows it as a separate source instead of
> silently sweeping the whole project into the prompt.

Open **Details** briefly, then close it.

**0:48-1:18 — Browser evidence**

Open Browser, visit `example.com`, and attach the page.

> The Browser belongs to this chat and stays human-controlled. When I attach
> evidence, Plume records the exact page reference and shows what was shortened
> or redacted.

**1:18-1:48 — Memory and Library**

Open Library, select the prepared project note, and choose **Use in chat**.

> Project memory is scoped to this trusted project. Library makes the scope
> visible, and a memory only reaches the model when I deliberately add it.

**1:48-2:16 — Inspectable context and safe changes**

Return to Chat and show the File, Web, and Memory cards together. Remove and
restore one source if time permits.

> These are the three sources for the next turn. Normal chat is just normal
> chat. If I ask Plume to make changes, it can only draft a patch; I still choose
> Apply, and Plume keeps a reversible checkpoint.

Only click **Make changes** if a local model is selected and that exact path was
smoked before recording.

**2:16-2:46 — Build Week and Codex**

> During Build Week I used Codex with Sol from the GPT-5.6 family to extend an
> existing editor with explicit context, Browser evidence, Library, persistence,
> and lifecycle hardening. The local task index records about 2.7 billion direct
> processed tokens, including about 2.0 billion on Sol; that includes cached
> context, so it is not a billing number.

Show the About/Handbook surface or a simple end card with the repository and
category: **Developer Tools**.

**2:46-2:55 — Close**

> Plume makes agent context visible, inspectable, and reversible — locally.

## Do not claim

- agent-controlled Browser or computer use;
- broad shell or arbitrary tool execution;
- semantic retrieval or an autonomous multi-step loop;
- support beyond macOS Apple Silicon for this judge build;
- Developer ID signing or notarization unless completed later; or
- that processed-token totals equal paid or unique tokens.
