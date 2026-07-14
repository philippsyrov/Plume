# Browser screenshot evidence design

## Goal

Let a person take a screenshot of the visible sandbox Browser page, place that
immutable screenshot on the existing project-chat context shelf, and send it to
a local model only when the exact selected model proves that it supports
vision.

This is explicit evidence, not browser automation. The model cannot navigate,
click, capture, or retrieve pages. Remote page content still receives no Plume
IPC capability.

## Product contract

The Browser panel adds one plain action: **Use screenshot in chat**. It is
available only for a loaded page in a trusted project. The action captures the
currently visible WKWebView viewport, stores an immutable project-scoped PNG,
and adds an opaque screenshot reference to the project chat shelf.

The shelf shows provenance rather than raw bytes: page title, sanitized source
URL/host, capture time, pixel dimensions, and PNG size. The stored PNG never
crosses frontend IPC and is never exposed through a remote webview.

If the selected model cannot prove vision support, the screenshot remains
visible on the shelf but preview/send reports it as blocked with ordinary copy:
"This model cannot use screenshots." Plume does not infer capability from a
model name. For this slice, only Ollama's exact `/api/show` response containing
`vision` is authoritative. The current MLX-LM route remains honestly text-only.

## Authority and race boundaries

- Only the trusted main webview can request capture.
- Capture requires the same trusted project before and after the native
  snapshot callback.
- Capture uses the Browser store's page-generation ticket. Navigation, reload,
  close, or project switch during capture rejects the result and stores
  nothing.
- The native snapshot is taken from Plume's process-owned sandbox WKWebView via
  `takeSnapshotWithConfiguration`; no page-authored JavaScript participates.
- `chat.send` re-probes the exact Ollama model before stream registration. A
  stale frontend capability badge cannot authorize image delivery.
- The screenshot is attached only to the final user message in the Ollama
  request's `images` array. Existing text-only messages remain byte-compatible.

## Storage contract

Screenshot evidence lives below `.plume/browser-evidence/` beside text
evidence, with separate metadata JSON and PNG files. The opaque id uses a
distinct `bs_` prefix so text and image records cannot be confused.

Hard limits:

- visible viewport only;
- pixel dimensions must be non-zero and at most 4096 by 4096;
- one PNG at most 4 MiB;
- at most 25 screenshot records and 32 MiB of screenshot bytes per project;
- metadata at most 64 KiB;
- no symlinks or hardlinks for metadata or PNG files;
- atomic sibling-tempfile writes, with cleanup if the second write fails.

The metadata stores only version, id, sanitized URL, redacted bounded title,
capture time, width, height, PNG byte count, and SHA-256 digest. Read-time
validation repeats all limits, fully decodes the PNG, and verifies its stored
size and digest before prompt use.

## Prompt and manifest contract

The persisted shelf reference is only:

```json
{ "kind": "browserScreenshotEvidence", "evidenceId": "bs_..." }
```

Preview and send resolve it from the current trusted project. The exact
manifest records the immutable provenance fields plus the PNG digest; model
support is reported separately as ready or blocked during preview. Image bytes
are not folded into the text context byte budget. The
Ollama adapter receives bounded PNG bytes separately and base64-encodes them at
the transport edge.

Local chats reject screenshot references at the existing session boundary.
Forked/rewound project turns retain historical manifests, while the live shelf
continues to hold only opaque refs.

## Honest exclusions

No full-page screenshot, OCR, image editing, thumbnail IPC, automatic capture,
background browsing, agent-driven browser actions, macOS host control, or MLX
vision claim ships here. A later MLX/VLM slice may implement an image-capable
adapter behind the same exact capability gate.
