# Human Browser Workspace Design

**Status:** Approved by the active Browser Phase A goal.  
**Base:** `origin/main@56e53e00dca140f9b13eb42ea8fbde7f3920f6fe`.  
**Scope:** Human navigation only. No model, agent, scheduler, or hidden browser control.

## Outcome

Plume gains one simple Browser workspace that works with or without an open
project. The main Plume window owns the controls and visible state. Remote
content stays in the separately labelled `browser-sandbox` window whose lack of
Plume/Tauri authority was proved in PR #142.

This slice does not embed remote content into the trusted React tree. A future
multi-webview presentation may do that only if it preserves the same label and
capability boundary.

## Human flow

1. Open **Workspace views → Browser**.
2. Enter an absolute HTTP(S) URL, or a localhost URL, and press **Go**.
3. Public HTTP(S) opens immediately in the sandbox window.
4. An exact loopback origin such as `http://localhost:5173` first shows one
   compact confirmation: “Open localhost? This page can connect to an app
   running on this Mac.”
5. Confirming adds only that normalized origin to the current browser-window
   session. Closing the browser clears the set.
6. The workspace shows the authoritative current URL, loading/error state, and
   human Back, Forward, Reload, Show, and Close controls.

The control surface stays sparse. It does not show security jargon, AGENTS.md,
context manifests, traces, or agent controls. A small “Sandboxed window” label
is enough to explain why content opens separately.

## Global placement

Browser is not project-scoped. The Workspace views drawer enables Browser both
inside project mode and in simple-chat mode. Files, Knowledge, and Benchmarks
still require a project. Switching views never closes the browser; explicit
Close or closing the sandbox window does.

## Localhost boundary

Loopback classification reuses the Rust policy from PR #142: `localhost`,
subdomains of `.localhost`, `127.0.0.0/8`, and `::1`, without DNS.

`browser.sandboxOpen` gains an optional `approvedLoopbackOrigin`. For a
loopback target, the backend normalizes its origin and requires either:

- that exact origin already exists in the process-owned session allowlist; or
- the trusted main webview supplies the same exact normalized origin after the
  user confirms.

An approval for one port does not approve another port. Public targets reject a
spurious loopback approval. Page-authored top-level navigation to an unapproved
loopback origin is denied and surfaces a stable failure. Ordinary subresources
are still not filtered; docs must continue to state that limitation plainly.

## Commands

The trusted main webview receives five additional fixed-purpose commands:

- `browser.sandboxFocus({})`
- `browser.sandboxBack({})`
- `browser.sandboxForward({})`
- `browser.sandboxReload({})`
- the existing open command accepts `approvedLoopbackOrigin?: string`

Back and Forward may use fixed internal history expressions; no arbitrary
JavaScript string crosses IPC. All commands repeat the exact-main caller guard.
Missing windows reject `NotFound`. Close stays idempotent.

## State and refresh

The frontend polls `browser.sandboxState` only while the Browser workspace is
mounted: quickly while loading and slowly while idle. It cancels timers and
ignores stale responses on unmount. The Rust state remains authoritative.

`title` remains `null` in this slice. Tauri does not associate title callbacks
with a document, so the UI uses the current hostname/URL instead of guessing.

## Error copy

Stable backend details map to short product copy:

- invalid or oversized URL → “Enter a full http:// or https:// address.”
- blocked scheme/credentials → “That address is not allowed.”
- loopback approval required → show the localhost confirmation, not an error
- navigation failure → “The page could not be opened.”
- missing browser window → “Open a page first.”

Raw internal error details never become the primary UI copy.

## Verification

- Rust policy/store/command tests for exact-origin approvals, session clearing,
  page-authored denial, and every fixed command guard.
- Frontend tests for public navigation, localhost confirmation/cancel/confirm,
  retry/error/loading, global drawer reachability, polling cleanup, and no agent
  controls.
- Full verifier, GitHub verify, gitleaks, and exact-head independent review.
- Packaged smoke against a tiny `/private/tmp` localhost fixture: open Browser,
  confirm the exact origin, observe the external sandbox window, navigate,
  reload, close, and confirm the main Plume window remains healthy.

## Non-goals

No screenshots, excerpts, prompt evidence, DOM extraction, downloads, popups,
tabs, bookmarks, saved history, persistent localhost permissions, agent actions,
background navigation, browser scheduling, or macOS host control.
