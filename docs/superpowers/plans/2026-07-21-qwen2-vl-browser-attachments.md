# Qwen2-VL And Browser Attachments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one resource-conscious Plume-managed Qwen2-VL vision model, real screenshot inputs in chat and bounded research, and a clean Browser attachment handoff.

**Architecture:** Keep all image authority in Rust. The frontend sends existing opaque Browser screenshot references; Rust re-resolves bounded PNG bytes and only the Qwen2-VL MLX-VLM route serializes those bytes into the final user message. Generalize the fixed catalog and managed server launcher just enough for two reviewed catalog models while enforcing one active catalog runtime at a time.

**Tech stack:** Tauri 2, Rust 2021, React 19, TypeScript, Vitest, MLX-LM, MLX-VLM.

---

### Task 1: Pin the product contract with failing tests

**Files:**
- Modify: `src-tauri/src/chat/mlx_lm_tests.rs`
- Modify: `src-tauri/src/commands/chat/send_tests.rs`
- Modify: `src-tauri/src/research/model_tests.rs`
- Modify: `src-tauri/src/providers/catalog_tests.rs`
- Modify: `src/features/model-picker/ModelChooser.test.tsx`
- Modify: `src/features/browser/BrowserPanel.test.tsx`
- Modify: `src/features/chat/ContextShelf.test.tsx`

- [ ] Add a failing OpenAI multimodal request-shape test that pins PNG data URLs to the final user turn only.
- [ ] Add failing capability/routing tests proving only fixed Qwen2-VL accepts screenshots and its handle must match.
- [ ] Add failing catalog and chooser tests for the exact Qwen2-VL entry and action labels.
- [ ] Add failing Browser/menu and context-chip motion tests, including reduced-motion-safe semantics.
- [ ] Run the focused tests and record the expected failures.

### Task 2: Add the fixed Qwen2-VL catalog model and runtime kind

**Files:**
- Modify: `src-tauri/src/providers/catalog_manifest.json`
- Add: `src-tauri/src/providers/catalog_download_manifest_qwen2_vl.json`
- Modify: `src-tauri/src/providers/catalog.rs`
- Modify: `src-tauri/src/providers/catalog_download.rs`
- Modify: `src-tauri/src/commands/providers_catalog_download.rs`
- Modify: `src-tauri/src/commands/providers.rs`
- Modify: `src-tauri/src/providers/mlx_lm/process_launch.rs`
- Modify: `src-tauri/src/providers/mlx_lm/process.rs`
- Modify: `src/lib/api/providers.ts`

- [ ] Add `qwen2-vl-2b-instruct-4bit` with exact repository, revision, license, byte total, and fixed file hashes.
- [ ] Generalize fixed-manifest lookup, install receipts, removal, and catalog start without accepting arbitrary repositories.
- [ ] Add an explicit `mlx-vlm` managed runtime kind using the bundled interpreter and loopback-only server arguments.
- [ ] Stop the other fixed catalog runtime before starting a new one.
- [ ] Run focused Rust catalog, download, launcher, and command tests.

### Task 3: Send real screenshot bytes to Qwen2-VL chat

**Files:**
- Modify: `src-tauri/src/chat/mlx_lm.rs`
- Modify: `src-tauri/src/commands/chat/vision.rs`
- Modify: `src-tauri/src/commands/chat/send_route.rs`
- Modify: `src-tauri/src/commands/chat/send.rs`

- [ ] Serialize bounded PNG bytes as OpenAI multimodal `image_url` content on only the final user message.
- [ ] Route fixed Qwen2-VL through the MLX-VLM handle and reject screenshots for Apple, Qwen Coder, generic MLX, or mismatched handles.
- [ ] Preserve text-only MLX-LM behavior byte-for-byte when no images are attached.
- [ ] Run the focused chat adapter and command tests.

### Task 4: Carry screenshots through bounded research

**Files:**
- Modify: `src-tauri/src/research/evidence.rs`
- Modify: `src-tauri/src/research/model.rs`
- Modify: `src-tauri/src/research/run.rs`
- Modify: `src-tauri/src/commands/research.rs`
- Modify: `src-tauri/src/research/evidence_tests.rs`
- Modify: `src-tauri/src/research/model_tests.rs`
- Modify: `src-tauri/src/commands/research_tests.rs`

- [ ] Resolve text and screenshot evidence from the same exact persisted chat shelf.
- [ ] Keep text-source provenance and citation checks exact; screenshots are supplementary visual evidence, not fabricated citations.
- [ ] Pass bounded image bytes only to the Qwen2-VL research model port and keep the existing 13-turn/26-call/recovery limits.
- [ ] Reject screenshot research on Apple and Qwen Coder with an ordinary typed error.
- [ ] Run the focused research tests.

### Task 5: Add the chooser row and clean Browser-to-chat handoff

**Files:**
- Modify: `src/features/model-picker/useModelCatalog.ts`
- Modify: `src/features/model-picker/ModelChooser.tsx`
- Modify: `src/features/model-picker/useModelCatalog.test.tsx`
- Modify: `src/features/model-picker/ModelChooser.test.tsx`
- Modify: `src/features/browser/BrowserPanel.tsx`
- Modify: `src/styles/layout/browser.css`
- Modify: `src/features/chat/ContextShelf.tsx`
- Modify: `src/styles/` context-shelf stylesheet

- [ ] Generalize the catalog UI lifecycle to Qwen Coder and Qwen2-VL without duplicating download races.
- [ ] Render `Qwen2-VL 2B` with short ordinary-language copy and explicit download/use states.
- [ ] Reset native WebKit button appearance in the Browser attachment menu and reuse Plume tokens/icons.
- [ ] Animate a newly attached screenshot chip from the Browser side into the chat shelf; disable movement under `prefers-reduced-motion`.
- [ ] Run focused frontend tests and typecheck.

### Task 6: Bundle, document, and verify the exact product

**Files:**
- Modify: `scripts/mlx-runtime-requirements.in`
- Modify: `scripts/mlx-runtime-requirements.lock`
- Modify: relevant runtime/package scripts and tests
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/MODEL_PROVIDERS.md`
- Modify: `docs/SMOKE_TESTING.md`
- Modify: frontend/Rust domain maps when ownership changes

- [ ] Pin and lock the reviewed MLX-VLM runtime through the project-local build path.
- [ ] Download and hash-verify the exact Qwen2-VL manifest only after the downloader passes focused tests.
- [ ] Run `npm run test`, `npm run typecheck`, and focused Rust tests.
- [ ] Run `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`, pre-commit, and gitleaks.
- [ ] Build/package one exact Plume app and smoke chooser, screenshot drop, Browser attach motion, chat vision, research vision, switching/unload, and Markdown export.
- [ ] Run a findings-only exact-head review and prepare a PR without merging.
