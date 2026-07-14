# Browser Phase A: capability-isolation proof

**Status:** approved for implementation under the active Browser Phase A goal

**Base:** `origin/main@ed5e4dfe3d686fcbe7b5dc61d89a1e1a7c452c1e`

## Outcome

Plume gains a real, testable trust boundary for remote browser content before a
normal-user Browser workspace is exposed. The trusted main frontend keeps its
current Tauri authority. A separately labelled `browser-sandbox` webview gains
none of Plume's application, core, event, filesystem, process, or plugin
authority.

This slice establishes the backend window lifecycle and URL-policy foundation
that the next Browser slice will drive. It does not add the user-facing URL bar
or claim that browser evidence can reach a prompt.

## Why this must precede Browser UI

Plume's current capability targets `windows: ["main"]`. Tauri applies a
window-scoped capability to every webview inside that window. Embedding remote
content in the main window without changing that selector would therefore make
the boundary depend on Tauri's remote-origin guard alone and would give the
remote webview any core permissions inherited through the main window.

Custom commands also need an explicit application manifest. Without one,
registered application commands are not represented as individually auditable
permissions. Browser Phase A changes the default from implicit local-webview
authority to an explicit list granted only to the trusted main webview.

## Threat model

Treat every document loaded by `browser-sandbox` as attacker-controlled. It may
run arbitrary JavaScript, redirect, submit forms, request popups or downloads,
change its title, and attempt to construct raw Tauri IPC messages.

The boundary must remain safe if the page:

- imports or reconstructs Tauri's JavaScript invoke surface;
- calls any Plume application command, including the harmless `ping` probe;
- calls Tauri core/event commands;
- redirects to `tauri:`, `file:`, `data:`, a custom protocol, or malformed URL;
- requests `window.open` or a download;
- reuses stale labels or races open and close requests.

Normal web behavior such as setting the document title or issuing ordinary
HTTP(S) subresource requests is not Plume authority. Host-network policy beyond
top-level navigation is a later hardening topic; this slice grants no backend
HTTP client or filesystem bridge.

## Architecture

### 1. One application-command registry

Add one Rust source file containing the complete snake-case command-name list.
Both `build.rs` and crate tests consume this list.

`build.rs` switches from `tauri_build::build()` to
`tauri_build::try_build(...)` with
`AppManifest::new().commands(APP_COMMANDS)`. Tauri then generates
`allow-<command>` and `deny-<command>` permissions for every registered Plume
command.

The trusted capability lists all generated `allow-*` permissions plus the
existing event/listener permissions. A parity test parses the
`generate_handler!` block and fails when its command set differs from
`APP_COMMANDS`. Adding a future command without adding its permission therefore
fails verification instead of silently broadening or breaking authority.

### 2. Capabilities bind to webview labels

Replace `windows: ["main"]` with `webviews: ["main"]` in the trusted
capability. Do not add a wildcard and do not add a remote URL pattern.

`browser-sandbox` matches no capability file. It receives zero permissions.
There is deliberately no empty sandbox capability: matching no capability is
the clearest representation of no IPC authority.

### 3. Backend-owned sandbox lifecycle

Add a focused `browser` module and three commands:

- `browser_sandbox_open({ url }) -> BrowserSandboxState`
- `browser_sandbox_close({}) -> BrowserSandboxState`
- `browser_sandbox_state({}) -> BrowserSandboxState`

The state reports only lifecycle and visible navigation facts needed by the
next slice: `open`, `windowLabel`, `requestedUrl`, `currentUrl`, `title`,
`loading`, and the latest typed navigation failure. It never returns page HTML,
cookies, storage, JavaScript values, or screenshots.

Only one sandbox window exists. Re-opening navigates or focuses the existing
window; it never creates an unbounded set of windows. Closing is idempotent.
Window destruction clears process state so relaunch and stale-label behavior
are deterministic.

The stable `WebviewWindowBuilder` API is used for this proof. A later embedded
workspace may move to Tauri's multi-webview API only after separately accepting
its unstable-feature cost. The security boundary is label-based and survives
that presentation change.

### 4. Navigation policy

`browser_sandbox_open` accepts an absolute URL only. The shared policy:

- allows `https`;
- allows `http`;
- rejects credentials in the authority component;
- rejects missing hosts, malformed ports, fragments containing control
  characters, and every non-HTTP(S) scheme;
- classifies loopback hosts (`localhost`, subdomains of `.localhost`,
  `127.0.0.0/8`, and `::1`) explicitly for the next slice's policy;
- never resolves DNS in this slice and therefore never pretends a hostname is
  non-local based on a one-time lookup.

The webview's `on_navigation` hook applies the same scheme/shape validation to
every top-level navigation. Slice 1 permits both validated public and loopback
HTTP(S) URLs because the trusted main frontend explicitly submitted the initial
target; slice 2 adds the user-facing loopback transition/confirmation policy.

`on_new_window` always returns `Deny`. `on_download` always returns `false`.
The window is incognito and leaves clipboard access, autofill, extensions, and
devtools disabled. JavaScript remains enabled because modern local web-app
testing is a Phase A requirement.

### 5. Observation is not authority

Rust-owned callbacks may observe page-load start/finish, current top-level URL,
and document title. They update `BrowserSandboxState`; the page cannot call
those callbacks or emit arbitrary Plume events. This one-way observation path
is the foundation for visible navigation state in slice 2.

No initialization script is injected. No message handler, event bridge,
clipboard bridge, page-evaluation command, or content extraction exists in
this slice.

## IPC boundary

The three browser commands are normal application commands and therefore appear
in the main webview's explicit capability. Their handlers additionally reject
calls unless the invoking webview label is exactly `main`. This command-layer
check is defense in depth on top of Tauri's runtime authority.

Opening a browser sandbox does not require a trusted project. Browser is a
workspace-level surface, and local web-app testing must also work before a
project is opened. It does require a visible main window invocation; no
background or agent caller exists.

Errors use stable typed reasons: `invalidUrl`, `schemeBlocked`,
`credentialsBlocked`, `windowCreateFailed`, and `navigationFailed`. Raw system
paths, cookies, page content, and Tauri invoke keys never appear in responses.

## Verification

### Rust unit and integration tests

- URL policy accepts public HTTPS, localhost, `.localhost`, IPv4 loopback, and
  IPv6 loopback while classifying loopback honestly.
- URL policy rejects relative URLs, credentials, non-HTTP(S) schemes,
  malformed hosts/ports, and control characters.
- popup and download policies are pinned to deny.
- lifecycle planning is single-window, idempotent-close, and stale-state safe.
- command registry and `generate_handler!` parity is exact.
- capability JSON targets only `webviews: ["main"]`, has no wildcard, and has
  no `remote.urls` grant.
- every registered command has exactly one generated `allow-*` permission in
  the main capability.

### Runtime-authority proof

Use Tauri's `MockRuntime` and `get_ipc_response`, built with Plume's generated
context and real capability configuration:

1. local origin + webview `main` + `ping` succeeds;
2. local origin + webview `browser-sandbox` + `ping` is denied;
3. remote origin + webview `browser-sandbox` + `ping` is denied;
4. sandbox access to a granted main core/event command is denied;
5. a newly registered command omitted from the registry makes the parity test
   fail.

This tests the authority decision directly, not merely the JSON shape.

### Packaged smoke posture

There is no normal-user Browser UI in this slice, so a visual workflow claim is
not made. The packaged build must still launch and the trusted main UI must
retain ordinary IPC behavior. The next slice, which exposes navigation, must
run the hostile local-page packaged smoke and verify the actual system webview.

## Documentation

Update `SAFETY.md`, `IPC_ROADMAP.md`, `AGENT_OPERABILITY.md`, `ROADMAP.md`, and
`FEATURE_INVENTORY.md` with an honest floor:

- explicit main-webview command authority is shipped;
- the sandbox lifecycle and authority proof are shipped;
- human navigation UI, actual packaged hostile-page proof, evidence capture,
  and agent actions are not shipped.

## Non-goals

- No URL bar, back/forward/reload controls, tabs, history, bookmarks, or saved
  cookies.
- No screenshot, DOM text, excerpt, page-source, or PDF capture.
- No drag/drop from Browser into chat.
- No agent-authored click/type/scroll actions.
- No hidden browsing, background navigation, scheduling, or automatic
  retrieval.
- No macOS accessibility or host-control permission.
- No remote URL receives any Plume capability.

## Slice handoff

After independent exact-head review and merge, slice 2 builds the calm
human-controlled Browser workspace on these commands. It owns visible
navigation, loading/error state, back/forward/reload, the explicit loopback
transition policy, and packaged hostile-page smoke. Slice 3 then defines bounded
page/screenshot/excerpt evidence and extends the existing typed context shelf.
