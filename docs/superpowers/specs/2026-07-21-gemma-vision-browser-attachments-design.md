# Gemma Vision And Browser Attachments Design

## Goal

Add one low-memory, Plume-managed MLX vision model that can answer ordinary
chat and bounded research turns from screenshots, while making Browser
attachments feel like they visibly move into chat instead of disappearing.

## Model and runtime

- Add **Gemma Vision 4B** to the existing model chooser.
- Pin `mlx-community/gemma-3-4b-it-4bit` to one reviewed revision with an exact
  file, size, and SHA-256 manifest.
- Download weights only after the user presses **Download**. Keep the runtime
  in the app bundle and the weights in Plume's Application Support directory.
- Bundle a pinned `mlx-vlm` runtime beside the existing `mlx-lm` packages.
  Release builds never fall back to PATH Python or download dependencies at
  model start.
- Run the VLM on loopback only, with trust-remote-code disabled and one bounded
  owned process. Starting one catalog model stops the other catalog-owned
  model so Plume does not keep Qwen and Gemma resident together.
- Selecting another provider unloads Gemma. Cancellation, deadline, bounded
  logs, shutdown, and exact-handle rules match the existing MLX supervisor.

## Image authority and data flow

- Support two explicit image entry paths: dropping a supported image into the
  chat composer, and Browser **Attach -> Visible screenshot**.
- The frontend stores only the existing opaque typed image reference. Rust
  re-resolves the exact bytes through the current ownership, size, MIME,
  redaction, and session gates before a model request.
- Gemma receives the image through the owned MLX-VLM adapter only on the final
  user message. Page text and screenshots may be attached together.
- Text-only Apple and Qwen selections continue to fail closed before sending
  an image. No OCR fallback and no implied Browser navigation authority.
- Bounded research accepts exact Browser text and screenshot captures from the
  owning persisted chat. Screenshot summarization is available only through a
  vision-capable adapter; text-only providers return an ordinary actionable
  error without mutating the accepted source set.
- Citation projection remains provenance-only. A Browser screenshot keeps its
  exact URL and title so the final answer can expose the same source link.

## Model chooser

The chooser remains one calm page with three compact rows:

1. Apple On-Device
2. Qwen Coder 1.5B
3. Gemma Vision 4B

Gemma uses the same absent, downloading, verifying, starting, running,
selected, failed, retry, remove, and Details states as Qwen. The compact row
says **Image + text**. Technical source, license, revision, and errors stay in
Details. The chooser never claims the model is ready before the backend has
verified its receipt and started the exact managed handle.

## Browser attachment interaction

- Replace the raw grey browser menu appearance with the existing Plume
  surface, typography, spacing, hover, focus, and icon language.
- Keep three ordinary choices: **Selected text**, **Readable page text**, and
  **Visible screenshot**.
- On success, the Browser item moves toward the chat composer, then settles as
  the existing context chip with a short scale/fade confirmation. The
  underlying attachment is committed before the success animation completes.
- Dragged images use the same chip-entry animation.
- `prefers-reduced-motion: reduce` replaces travel and scale with a short
  opacity change. Keyboard focus returns to **Attach**, and the existing
  Browser status message remains available to assistive technology.
- Capture errors remain visible and never play the success animation.

## Error handling

- Model download, receipt, runtime identity, start, and image-send failures are
  typed and retryable. Raw local paths and stderr remain hidden from ordinary
  UI copy and available only in bounded Details where already permitted.
- Oversized or unsupported images fail before provider transport and stay in
  the composer for correction or removal.
- Starting Gemma cannot silently fall back to Qwen, Apple, Ollama, or a text
  route. Starting Qwen cannot reuse a Gemma handle.
- A failed attempt to stop the previous catalog model blocks the replacement
  start rather than leaving two resident catalog processes.

## Verification

- Frontend tests cover the third chooser row, all download/start states,
  image drop, Browser menu styling hooks, successful entry animation, reduced
  motion, keyboard flow, and visible errors.
- Rust tests cover exact Gemma manifest parsing, receipt isolation, managed
  model exclusivity, MLX-VLM launch identity, image request serialization,
  text-only rejection, cancellation, and shutdown.
- Package tests prove the pinned runtime contains both MLX-LM and MLX-VLM and
  contains no model weights.
- Packaged smoke downloads and verifies Gemma, attaches one Browser screenshot
  and one page-text capture, produces a cited answer, accepts one dropped local
  image, switches away, and proves the Gemma process exits.

## Explicit non-goals

- No automatic web search, URL fetching, Browser control, OCR fallback,
  audio/video input, arbitrary model catalog, broad tools, shell execution,
  semantic retrieval, or computer-use emission.
