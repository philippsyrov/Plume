# Plume Handbook

Plume is a private, local-first AI workspace for chatting, browsing, and
working with a folder on your Mac. If you know how to use ChatGPT or email, you
already know the basic interaction: choose where you want to work, type a
message, and review the answer.

Plume is still early. This guide separates what works today from what is only
planned. For the repository-level evidence behind every status claim, see the
[feature inventory](FEATURE_INVENTORY.md).

## Start Here

The sidebar has four main actions:

- **New chat** starts a conversation.
- **Search** finds text in saved chats.
- **Library** opens memories and project notes.
- **Open project** gives Plume access to one folder after you review and trust
  it.

**Settings** contains local models, Library editing, archived chats, and
advanced project controls. **Help** is the small question-mark button beside
Settings and opens a short offline guide inside the app.

**Workspace views** in the top bar contains tools only: **Files**, **Browser**,
and **Benchmarks**. Chat and Library navigation stay in the sidebar, so the
same destination never appears in two menus.

Plume does not contact a cloud model by default. Open **Model** in the top bar
to use Apple's on-device model when this Mac reports it available, or explicitly
download fixed Qwen Coder or Qwen2-VL.

## Chat Or Project?

When a project is open, **New chat** asks you to choose **Chat** or **Project**.

| Choose | Best for | What Plume can use |
| --- | --- | --- |
| **Chat** | Questions, explanations, writing, and web research | The conversation, Browser evidence you explicitly attach, and **About you** memories you explicitly attach |
| **Project** | Understanding or changing files in one folder | Everything above, plus the trusted folder, Project instructions, project memory, topics, and reviewed patch actions |

A Chat never quietly gains access to the open project. A Project does not run
arbitrary commands or change files merely because you asked. Today, file
changes use a visible diff that you explicitly apply.

### Worked example: ask a general question

1. Click **New chat**.
2. Choose **Chat** if Plume asks which kind.
3. Open **Model** and choose an available model.
4. Type your question and send it.
5. Click **Stop** if you want to end a streaming reply early.

The conversation is saved locally. Use its `...` menu to rename, archive,
delete, continue, or rewind it. Manage archived local and project chats
together under **Settings → Archived**.

### Worked example: ask about a file

1. Click **Open project**, choose a folder (or drop one from Finder), and review
   the trust prompt. **Enter path instead** keeps pasted paths available.
2. Start a **Project** chat.
3. Open a text file in **Files**.
4. Select the lines you care about, then choose **Use selection in chat**.
5. Ask Plume to explain or change those lines.

The context shelf shows what is ready for the next message. After a message is
accepted, that turn keeps a permanent record of the exact sources that reached
the prompt.

## Choose And Start A Local Model

The top-bar **Model** chooser is the normal setup path and works before a
project is open. Plume-managed MLX is the primary open-model path on Apple
Silicon. Apple On-Device is a separate system adapter, Ollama is a compatibility
path, and LM Studio and llama.cpp can be discovered but do not have Plume chat
adapters.

### Apple On-Device

Open **Model** and choose **Use Apple Model**. Plume asks the bounded helper for
the host's real Foundation Models status. Unsupported macOS, an ineligible
device, disabled Apple Intelligence, or a model that is not ready stays visible
as a disabled state. Apple generation is on-device only; Plume does not use
Private Cloud Compute and never silently switches an Apple message to Qwen.

### Qwen Coder 1.5B

Open **Model** and choose **Download** on **Qwen Coder 1.5B**. This is a fixed
catalog download: an Apache-2.0 checkpoint at a pinned revision with fixed file
sizes and hashes. Download is explicit, cancellable, resumable, and verified
before installation. After it is ready, choose **Use Qwen**; Plume starts it
with the bundled MLX-LM runtime and selects the exact running handle.

The runtime is inside the packaged app, but the roughly 880 MB model snapshot
is not. Its weights live in Plume's Application Support data and survive app
updates. Plume never turns the chooser into an arbitrary model downloader.

### Qwen2-VL 2B

Choose **Download** on **Qwen2-VL 2B**, then **Use Qwen2-VL** after the
pinned download verifies. Qwen2-VL runs through the bundled MLX-VLM runtime and can
inspect exact PNG screenshots captured from Plume's human-controlled Browser and
attached to the chat. External Finder image drop is not shipped. It does not
navigate the Browser. Research may use
screenshots for visual context, but still requires Browser text for citations.

Qwen2-VL's 13-file snapshot is 1,261,855,962 bytes and lives in Application
Support rather than inside Plume. Starting Qwen Coder or Qwen2-VL stops the
other fixed catalog model first, so the two do not compete for inference memory.

### Plume-managed MLX

1. Open and trust a project.
2. Put an already-downloaded compatible model folder in the configured Plume
   model directory. By default this is `plume-models/` inside the project.
3. Open **Settings** and find **Local models**.
4. Click **Start** beside the model. A successful start also selects it.
5. Close Settings and send a message.

This advanced path does not install dependencies or download models. The
current source candidate contains the verified MLX-LM/MLX-VLM runtime for fixed
Qwen Coder and Qwen2-VL catalog entries; the existing v0.1.0 public artifact is the
earlier Qwen-era release. Arbitrary folders still require project trust and
compatible weights.
If a model cannot start, its row shows the failure and lets you try again.

### Ollama

Start Ollama separately, then select one of its available models in Plume. If
the daemon is unavailable, Plume keeps the composer usable for drafting but
disables Send and offers **Recheck**.

## Work Inside A Project

Opening a folder does not immediately grant project authority. Plume first
shows what it found and asks you to trust that folder. Trust applies only to
that project.

After trust, a Project chat can use:

- a file or selected line range you place on the context shelf;
- **Project instructions** from the root instruction file;
- bounded project memory and curated topics;
- Browser evidence captured by you;
- a proposed unified diff that you review before applying.

Technical names, paths, byte counts, and exact manifests stay available under
**Details**. They are evidence, not extra authority.

## Browse Beside A Task

Every saved Chat or Project owns its own Browser workspace. Open **Workspace
views**, then choose **Browser**. The default split keeps the conversation and
page side by side.

The Browser provides tabs, an address field, **Back**, **Forward**, **Reload**,
and **Expand Browser**. Expanded Browser uses the full canvas by default.
Choose **Show chat** to pull up its compact task composer and **Hide chat** to
return to an uninterrupted page. **Return to split view** restores the
side-by-side layout.

Plume restores admitted top-level URLs, tab order, the active tab, and layout
for that chat. It does not promise to restore form contents, scroll position,
or a page's private JavaScript state. If a saved address had sensitive-looking
query or fragment data, Plume stores only a safer form and asks you to **Reopen
page** manually.

### Attach web evidence

Navigation alone never adds a page to a prompt. Open **Attach** and choose one
of these explicit actions:

- **Selected text**;
- **Readable page text**;
- **Visible screenshot**.

The evidence belongs to the chat that captured it. A visible screenshot means
the current Browser viewport, not the full page. Screenshot sending requires
the fixed Qwen2-VL MLX-VLM model or an Ollama model that Plume has freshly
verified as vision-capable. Text-only and unverifiable models fail closed.

### Worked example: test a local web app

1. Open a trusted Project chat and its Browser.
2. Enter the exact `localhost` or `127.0.0.1` address.
3. Review the origin shown by Plume and choose **Open**.
4. Inspect the page, then attach selected text, readable text, or a visible
   screenshot.
5. Ask the Project chat about the attached evidence.

Localhost approval is limited to that exact origin and live Browser session.
A normal Chat cannot approve project-localhost access.

## Ask For Research

This Stage A flow is implemented but remains partial. In a saved Chat or Project,
first attach between 1 and 10 **Selected text** or **Readable page text** Browser
captures. Select Apple On-Device, fixed Qwen Coder, or fixed Qwen2-VL, then send a
narrow chat request such as `Research what we know about feathered dinosaurs`.
With Qwen2-VL selected, you may additionally attach exact Browser screenshot PNGs;
at least one Browser text capture is still required for citation provenance.

Plume summarizes only the exact attached Browser evidence. Screenshots may add
visual context for Qwen2-VL, but only Browser text records become citation sources.
It does not search the web, fetch URLs, steer the Browser, read arbitrary project
files, or add memory, topics, or links. Progress and **Stop research** stay
visible. A completed note appears as a normal assistant reply. Its source links
open the owned, human-controlled Browser; they do not grant Browser authority to
the model. Send `Export this as Markdown` only when you want a file. Plume then
opens the native macOS save panel and adds one Markdown attachment to the chat;
the page never receives or chooses a filesystem path.

Citation checks only confirm that markers point to records in the artifact's
exact source bundle. They do not prove that a claim is true or that the source
is relevant. Review the answer and its sources before using it.

## Use Library And Memory

Library has four user-facing sources:

- **About you** — app-private memories available without a project;
- **This project** — memory stored only in the trusted project;
- **Topics** — curated project notes;
- **Connections** — exact stored links and backlinks.

Use **Settings → Library** to create, edit, or forget memories. Library itself
is the calmer reading and search surface.

About-you memory is never added to a prompt automatically. Click or drag **Use
in chat** to attach one exact entry to either a Chat or Project. Project memory
can contribute a small bounded ambient summary to a Project chat and can also
be attached explicitly. Topic links and backlinks are organization metadata
only: opening a connection does not add anything to a prompt.

Library search is lexical text search within the selected visible source. It
is not semantic retrieval and does not search every project on the Mac.

### Worked example: attach one preference

1. Open **Settings → Library → About you** and remember a short preference.
2. Open **Library → About you**.
3. Select the entry and choose **Use in chat**.
4. Confirm one User memory item appears on the destination chat's shelf.
5. Send the message.

Saving or merely viewing the entry never attaches it.

## Continue, Rewind, And Find Chats

Open a chat's `...` menu:

- **Continue in new chat** copies the completed conversation into a new saved
  chat and records where it came from.
- **Rewind into new chat...** creates a new chat ending at the turn you choose.

The original chat is unchanged. The new chat begins with an empty live context
shelf and a fresh Browser workspace, while copied historical turns keep their
accepted-source records. The short menu stays compact; open **About Continue
and Rewind** there when you need the safety explanation. Plume does not yet
compare or merge branches.

Use **Search** or `Command-K` to search saved titles and transcript text. Local
and project results remain visibly separate. Archived chats remain searchable.

## Review, Apply, And Revert A Change

In a trusted Project chat, choose **Propose diff** when you want a code change
rather than a prose answer.

1. Ask for a small change.
2. Read the rendered diff and validation result.
3. Click **Apply** only if the change is correct.
4. Plume checks the old file contents, creates a checkpoint, and writes the
   change atomically.
5. Click **Revert** if you want to restore that checkpoint. Revert stops if the
   files have drifted since Apply.

This is a patch-only safety loop. It is not permission for arbitrary file
writes or shell commands. Plume's single-step agent surface is also limited to
one trusted MLX turn; a production multi-step read/edit/test/fix agent is not
available yet.

## Permissions And Privacy

- **Project trust** grants bounded access to one chosen folder. It does not
  grant access to the rest of the Mac.
- **Context is visible.** Files, memories, topics, and Browser captures reach a
  prompt through their documented project contract or a visible attachment.
- **Secrets are redacted or blocked** on supported text paths. Screenshot
  pixels cannot be text-redacted, so capture remains an explicit human action.
- **Browser pages cannot call Plume commands.** The embedded page runs in a
  separately labelled WebKit surface without application IPC authority.
- **No cloud calls by default.** Local providers may still follow their own
  configuration, and websites loaded in Browser use ordinary network access.
- **No hidden computer control.** Plume cannot currently click through websites
  or control macOS on behalf of the model.

## Troubleshooting

### Send is disabled

Open **Model** in the top bar. Use Apple when the host reports it available,
download/start Qwen Coder or Qwen2-VL, start an advanced managed MLX model from a trusted
project, or start Ollama separately and click **Recheck**.

### A context item says Blocked

The source may have moved, exceeded a cap, failed a path or secret check, or
belong to another project. Remove or restore that exact item. Plume will not
silently replace it with a nearby source.

### The context shelf is full

Remove an item before attaching another. Duplicate attachments emphasize the
existing item instead of consuming another slot.

### A Browser page did not restore exactly

Browser restoration reloads a recorded top-level address; it does not preserve
temporary page state. Use **Reload** or the visible **Reopen page** action. A
corrupt Browser workspace can be reset without deleting the chat transcript.

### A local site is blocked

Localhost navigation is available only in a trusted Project chat. Confirm the
exact origin when Plume asks. Approval ends with the live Browser session.

### A screenshot cannot be sent

Use fixed Qwen2-VL or a freshly verified Ollama vision model. Text-only and
unverifiable models fail closed before the message starts.

## Available Now And Planned

The [feature inventory](FEATURE_INVENTORY.md) is the status authority. This
table is the short user-facing view of the same boundary.

| Area | Available now | Planned, not available now | Evidence |
| --- | --- | --- | --- |
| Chat history | Saved local and project chats, search, archive, delete, Continue, and Rewind | Cross-device sync, export, branch comparison, and branch merge | [Inventory](FEATURE_INVENTORY.md) |
| Local models | Host-gated Apple On-Device chat, explicit verified Qwen Coder and Qwen2-VL downloads with bundled MLX-LM/MLX-VLM runtime, advanced managed MLX start/stop/chat, and Ollama streaming chat | Arbitrary catalog downloads and LM Studio or llama.cpp chat adapters | [Inventory](FEATURE_INVENTORY.md) |
| Project work | Trusted files, instructions, explicit context, validated patch Apply/Revert | A production multi-step coding loop, arbitrary shell execution, and broad tool/plugin authority | [Inventory](FEATURE_INVENTORY.md) |
| Browser | Per-chat WebKit tabs, split/expanded layout, restoration, explicit text and visible-screenshot evidence | Agent-driven browsing, Chromium, full-page capture, hidden browsing, and Browser sharing between chats | [Inventory](FEATURE_INVENTORY.md) |
| Library | About you, project memory, topics, lexical search, links/backlinks, explicit attachment | Semantic retrieval, automatic prompt selection, graph view, cross-project aggregation, and dreaming | [Inventory](FEATURE_INVENTORY.md) |
| Computer use | Visible controls that a person or external accessibility tool can operate | Plume-controlled browser actions and opt-in macOS host control | [Inventory](FEATURE_INVENTORY.md) |
| Automation | No scheduled automation destination is shipped | Schedules, permissions, pause/resume, run status, and run history | [Inventory](FEATURE_INVENTORY.md) |
| Appearance | Light-first System, Light, and Dark choices in Settings | Custom foreground and background colors | [UI style](UI_STYLE.md) |

For implementation detail, use the [documentation map](README.md). For future
ordering, use the [roadmap](ROADMAP.md).
