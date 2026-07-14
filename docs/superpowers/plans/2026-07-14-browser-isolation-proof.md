# Browser Isolation Proof Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan.

**Goal:** Prove that attacker-controlled HTTP(S) content can run in a separately labelled Plume webview without receiving any Plume IPC or Tauri core authority, while laying the backend lifecycle and navigation-policy foundation for the human Browser workspace.

**Architecture:** Replace Plume's implicit local-webview application-command authority with one explicit command registry shared by `build.rs`, runtime registration, capability tests, and generated permissions. Bind the trusted capability to the `main` webview label only. Add a backend-owned `browser-sandbox` window whose top-level navigation, popup, download, and lifecycle behavior is governed by small pure policy/state modules and whose Tauri callbacks can observe navigation state without creating a page-to-Plume bridge.

**Tech Stack:** Rust 2021, Tauri 2 stable `WebviewWindowBuilder`, `url` parsing through Tauri's re-export, Serde, existing `IpcRequest`/`IpcError`, JSON capability files, Rust unit tests, Tauri `MockRuntime`, project verifier.

## Global constraints

- Work from `origin/main@ed5e4dfe3d686fcbe7b5dc61d89a1e1a7c452c1e` on `codex/browser-isolation`.
- Keep the original linked worktree untouched; its icon changes are user-owned.
- Write tests before production behavior for each task.
- Use stable Tauri APIs only; do not enable the `unstable` multiwebview feature.
- Remote content receives no capability match, no initialization script, no message bridge, no filesystem/process/plugin access, and no app events.
- Browser commands accept calls only from the trusted `main` webview as defense in depth.
- Do not add frontend navigation, screenshots, extraction, prompt evidence, agent actions, hidden browsing, scheduling, or host control in this slice.
- Run focused tests after each task, then the full verifier, secret scan, packaged build/liveness checks, and exact-head review before merge.

### Task 1: Make application-command authority explicit and auditable

**Files:**

- Create: `src-tauri/src/app_commands.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Test: `src-tauri/src/app_commands.rs`

**Step 1: Add failing registry/capability parity tests**

Create `src-tauri/src/app_commands.rs` with tests that read `lib.rs` and `capabilities/default.json`. The initial test module should require:

```rust
#[test]
fn registered_handlers_match_the_application_manifest() {
    let registered = handler_names(include_str!("lib.rs"));
    assert_eq!(registered, APP_COMMANDS);
}

#[test]
fn trusted_capability_grants_every_application_command_once() {
    let capability: serde_json::Value = serde_json::from_str(include_str!(
        "../capabilities/default.json"
    ))
    .expect("default capability must be valid json");
    assert_eq!(capability["webviews"], serde_json::json!(["main"]));
    assert!(capability.get("windows").is_none());
    assert!(capability.get("remote").is_none());

    let permissions = capability["permissions"]
        .as_array()
        .expect("permissions must be an array");
    for command in APP_COMMANDS {
        let wanted = format!("allow-{command}");
        assert_eq!(
            permissions.iter().filter(|value| value.as_str() == Some(&wanted)).count(),
            1,
            "{wanted} must be granted exactly once"
        );
    }
}
```

The helper must extract only the comma-separated identifiers inside the one `tauri::generate_handler![...]` block, trim whitespace, and reject duplicates. It must not silently sort away runtime ordering drift.

**Step 2: Run the tests and confirm the expected failure**

Run:

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test app_commands -- --nocapture'
```

Expected: FAIL because `APP_COMMANDS` does not exist yet and the capability still uses `windows: ["main"]` without generated app-command permissions.

**Step 3: Add the complete command registry**

Define one ordered `pub const APP_COMMANDS: &[&str]` containing every current identifier in `generate_handler!`, starting with `ping` and ending with `agent_single_step`. Include the three browser commands only after Task 3 adds them.

Keep the source file free of Tauri handler values so `build.rs` can include it without linking the product crate:

```rust
pub const APP_COMMANDS: &[&str] = &[
    "ping",
    "project_open",
    // ...exact remaining generate_handler order...
    "agent_single_step",
];
```

**Step 4: Use the registry in the Tauri build manifest**

In `src-tauri/build.rs`, include the shared source and preserve `emit_build_identity()`:

```rust
#[path = "src/app_commands.rs"]
mod app_commands;

fn main() {
    emit_build_identity();
    let manifest = tauri_build::AppManifest::new()
        .commands(app_commands::APP_COMMANDS.iter().copied());
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to build Tauri application manifest");
}
```

Use the exact iterator/owned shape accepted by the installed `tauri-build` version; preserve the same semantic source of truth if its type signature requires `Vec<&str>` or `Vec<String>`.

**Step 5: Bind authority to `main` webview and grant explicit commands**

Change `src-tauri/capabilities/default.json` from:

```json
"windows": ["main"]
```

to:

```json
"webviews": ["main"]
```

Retain the three existing event/listener permissions, then add exactly one generated `allow-<snake_case_command>` permission for every `APP_COMMANDS` entry. Do not add `remote`, wildcard labels, or a `browser-sandbox` capability.

**Step 6: Compile, inspect generated permission names, and adjust only to generated truth**

Run:

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test app_commands -- --nocapture'
rg -n 'allow-(ping|project|chat|memory)' src-tauri/gen src-tauri/target -g '*.json' -g '*.toml'
```

Expected: tests PASS and generated identifiers match the capability exactly. If Tauri normalizes underscores, change both the test helper and capability to the generated identifiers rather than guessing.

**Step 7: Commit the authority registry**

```bash
git add src-tauri/src/app_commands.rs src-tauri/build.rs src-tauri/capabilities/default.json
git commit -m "feat: make main webview authority explicit"
```

### Task 2: Build the pure Browser URL and lifecycle policy with TDD

**Files:**

- Create: `src-tauri/src/browser/mod.rs`
- Create: `src-tauri/src/browser/policy.rs`
- Create: `src-tauri/src/browser/state.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline tests in `src-tauri/src/browser/policy.rs`
- Test: inline tests in `src-tauri/src/browser/state.rs`

**Step 1: Write failing URL-policy tests**

Pin these accepted cases and classifications:

```rust
https://example.com/path           => Public
http://example.com                 => Public
http://localhost:5173              => Loopback
http://app.localhost:3000/path     => Loopback
http://127.42.0.1:8080             => Loopback
http://[::1]:9000                  => Loopback
```

Pin typed rejection reasons for relative URLs, missing hosts, `file:`, `tauri:`, `data:`, `javascript:`, embedded username/password, malformed ports, NUL/control characters, and empty input.

**Step 2: Run the policy tests and confirm failure**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser::policy -- --nocapture'
```

Expected: FAIL because the browser policy module does not exist.

**Step 3: Implement the URL policy**

Expose:

```rust
pub enum BrowserNetworkTarget { Public, Loopback }

pub enum BrowserUrlError {
    InvalidUrl,
    SchemeBlocked,
    CredentialsBlocked,
}

pub struct ValidatedBrowserUrl {
    pub url: tauri::Url,
    pub target: BrowserNetworkTarget,
}

pub fn validate_browser_url(raw: &str) -> Result<ValidatedBrowserUrl, BrowserUrlError>;
```

Perform a raw control-character scan before parsing. Require `http` or `https`, a host, no username, and no password. Classify `localhost`, names ending exactly in `.localhost`, all `127.0.0.0/8`, and `::1` as loopback without DNS resolution.

Expose pure deny helpers used by callbacks and tests:

```rust
pub const fn allow_popup() -> bool { false }
pub const fn allow_download() -> bool { false }
```

**Step 4: Write failing lifecycle-state tests**

Test the state transitions rather than mocking the OS window:

- initial state is closed and contains no stale URL/title/error;
- opening records the requested/current URL and sets loading;
- navigation start/finish updates the current URL and loading flag;
- title updates are bounded and do not affect authority;
- close clears URL/title/loading/error;
- closing an already-closed state is idempotent;
- a navigation failure is typed and a subsequent successful open clears it.

**Step 5: Implement serializable lifecycle state**

Expose a process-owned store with poison-safe locking:

```rust
pub const BROWSER_SANDBOX_LABEL: &str = "browser-sandbox";

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSandboxState { /* exact visible fields from the design */ }

#[derive(Default)]
pub struct BrowserSandboxStore {
    inner: Mutex<BrowserSandboxState>,
}
```

Keep mutation methods crate-private and return cloned snapshots. Bound the observed title and error message lengths so a hostile page cannot grow process memory through repeated callbacks.

**Step 6: Run focused tests and commit**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser:: -- --nocapture'
git add src-tauri/src/browser src-tauri/src/lib.rs
git commit -m "feat: add sandbox browser policy state"
```

### Task 3: Add the backend-owned sandbox window lifecycle

**Files:**

- Create: `src-tauri/src/commands/browser.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app_commands.rs`
- Test: inline tests in `src-tauri/src/commands/browser.rs`

**Step 1: Write failing command-boundary and lifecycle-planning tests**

Use a small pure helper for the defense-in-depth caller check:

```rust
fn require_main_webview(label: &str) -> Result<(), IpcError>;
```

Tests must pin `main` as accepted and `browser-sandbox`, empty, and arbitrary labels as `Blocked` without leaking invocation keys. Add pure planning tests for:

- absent sandbox window => create one;
- existing sandbox window => navigate/focus it, not create a second;
- close absent => successful closed snapshot;
- destroy event => store reset.

**Step 2: Run the focused tests and confirm failure**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test commands::browser -- --nocapture'
```

Expected: FAIL because no browser commands are registered.

**Step 3: Implement the three command payloads and handlers**

Use the existing versioned IPC shape:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSandboxOpenPayload { pub url: String }

#[derive(Deserialize, Default)]
pub struct BrowserSandboxEmptyPayload {}
```

Every command takes `tauri::WebviewWindow`, `tauri::AppHandle`, managed `BrowserSandboxStore`, and `IpcRequest<...>` as needed. Check IPC version first, then caller label, then arguments/state.

Build the sandbox using `tauri::WebviewWindowBuilder` with:

- stable label `browser-sandbox`;
- validated initial `tauri::WebviewUrl::External` URL;
- incognito enabled;
- JavaScript enabled;
- devtools disabled;
- clipboard access disabled;
- browser extensions disabled where supported;
- autofill disabled where supported;
- `on_navigation` calling the same URL policy;
- `on_new_window` always denying;
- `on_download` always returning false;
- page-load and title callbacks updating Rust-owned state only;
- window destroy callback clearing state.

Do not inject initialization JavaScript or emit Tauri events to the page.

Map failures to stable typed `IpcError::BadArgument`/`Blocked` details for the first implementation, or add a focused Browser error response if that better matches the repo's existing wire conventions. The frontend is not consuming these commands yet, so do not widen the global error enum without a concrete benefit.

**Step 4: Register state and commands**

Manage `BrowserSandboxStore::default()` in `setup`. Import and add:

```rust
browser_sandbox_open,
browser_sandbox_close,
browser_sandbox_state,
```

to the `generate_handler!` block. Add the same names in the same order to `APP_COMMANDS` and add their generated `allow-*` identifiers to the `main` capability.

**Step 5: Run focused compile/tests and fix installed-API mismatches**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings'
```

Expected: PASS. If a builder method is target-gated or absent in the installed Tauri version, inspect the exact crate source and document the property that cannot be set; do not enable broad features or invent APIs.

**Step 6: Commit the lifecycle**

```bash
git add src-tauri/src/browser src-tauri/src/commands/browser.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src-tauri/src/app_commands.rs src-tauri/capabilities/default.json
git commit -m "feat: add isolated browser sandbox lifecycle"
```

### Task 4: Prove actual runtime authority decisions

**Files:**

- Create: `src-tauri/src/browser/authority_tests.rs`
- Modify: `src-tauri/src/browser/mod.rs`
- Modify: `src-tauri/src/lib.rs` only if a test-only builder helper is required

**Step 1: Add the real MockRuntime denial matrix**

Build a test app with Plume's generated context, actual capability configuration, and at least the `ping` handler. Use Tauri's public `test::mock_builder`, `MockRuntime`, and `get_ipc_response` APIs to send raw invoke requests for:

- local main webview + `ping` => `pong`;
- local `browser-sandbox` webview + `ping` => denied;
- remote `browser-sandbox` origin + `ping` => denied;
- `browser-sandbox` + one main-granted core/event command => denied.

The test must exercise Tauri's authority resolution, not call `ping()` or `require_main_webview()` directly.

**Step 2: Run and confirm the test initially exposes missing harness wiring**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser::authority_tests -- --nocapture'
```

Expected on the first run: compile/test failure until the real request builder, origins, labels, and response decoder match Tauri 2.11's test API.

**Step 3: Complete the smallest reusable test builder**

Factor app construction only if necessary:

```rust
#[cfg(test)]
fn test_builder() -> tauri::Builder<tauri::test::MockRuntime> {
    tauri::test::mock_builder().invoke_handler(tauri::generate_handler![ping])
}
```

Use generated context from the same crate so the test reads the production capability. Avoid a test-only permissive capability or `dangerous_disable_asset_csp_modification` shortcuts.

**Step 4: Run the authority matrix and broader Rust suite**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser::authority_tests -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test'
```

Expected: all four authority cases PASS and the existing suite remains green.

**Step 5: Commit the proof**

```bash
git add src-tauri/src/browser/authority_tests.rs src-tauri/src/browser/mod.rs src-tauri/src/lib.rs
git commit -m "test: prove browser sandbox has no ipc authority"
```

### Task 5: Document the shipped floor without overstating Browser capability

**Files:**

- Modify: `docs/SAFETY.md`
- Modify: `docs/IPC_ROADMAP.md`
- Modify: `docs/AGENT_OPERABILITY.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/SMOKE_TESTING.md` only if the existing packaged liveness procedure gains a browser-isolation check

**Step 1: Write the documentation assertions before prose edits**

Identify the canonical status sections in all five documents. Prepare exact statements for:

- shipped: explicit application-command manifest;
- shipped: trusted capability targets only webview `main`;
- shipped: `browser-sandbox` receives no capability match;
- shipped: backend single-window lifecycle and HTTP(S)-only top-level policy;
- verified in Rust: direct runtime denial matrix;
- not shipped: normal-user Browser navigation UI;
- not yet proven: packaged hostile-page behavior in the actual system webview;
- not shipped: screenshots/excerpts/context evidence, agent actions, automatic retrieval, host control.

**Step 2: Update canonical docs consistently**

Keep `PLUME_PROJECT_SPEC.md` unchanged unless its existing Browser Phase A contract is factually stale. Do not renumber or reuse campaign slice IDs. Use descriptive milestone names and shipped/candidate status labels.

**Step 3: Run docs verification and status scans**

```bash
npm run verify:docs
rg -n 'browser-sandbox|Browser Phase A|computer use|host control|screenshot|excerpt' docs/SAFETY.md docs/IPC_ROADMAP.md docs/AGENT_OPERABILITY.md docs/ROADMAP.md docs/FEATURE_INVENTORY.md
```

Expected: docs verifier PASS; every status claim agrees across the five files.

**Step 4: Commit docs**

```bash
git add docs/SAFETY.md docs/IPC_ROADMAP.md docs/AGENT_OPERABILITY.md docs/ROADMAP.md docs/FEATURE_INVENTORY.md docs/SMOKE_TESTING.md
git commit -m "docs: record browser isolation floor"
```

### Task 6: Verify, package-smoke, publish, and hold for exact-head review

**Files:**

- Modify tests/docs only if verification exposes a real defect
- Do not change product scope during cleanup

**Step 1: Run formatting and focused checks**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo fmt --all -- --check'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test browser -- --nocapture'
npm run typecheck
```

Expected: all PASS.

**Step 2: Run the full repository verifier**

```bash
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
```

Expected: all hard checks PASS; only the two pre-existing documentation soft-cap warnings remain unless the baseline changes.

If the sandbox blocks Vitest's loopback benchmark listener, rerun the same verifier with the already-approved elevated command rather than changing tests or omitting the check.

**Step 3: Build and launch the packaged app liveness smoke**

Use the repository's documented packaged smoke command from `docs/SMOKE_TESTING.md`. Verify:

- the desktop shell remains the packaged executable;
- the main UI launches;
- ordinary trusted-main IPC still responds;
- no normal-user Browser UI is claimed;
- no visual hostile-page claim is recorded before slice 2.

**Step 4: Review the exact diff for authority mistakes**

```bash
git diff origin/main...HEAD -- src-tauri/build.rs src-tauri/src/app_commands.rs src-tauri/src/lib.rs src-tauri/src/browser src-tauri/src/commands/browser.rs src-tauri/capabilities/default.json docs
git diff --check
git status --short
```

Explicitly search for wildcards, remote grants, injected scripts, emitted page events, clipboard/autofill/devtools enablement, unbounded window labels, and duplicate command registries.

**Step 5: Push and open one focused PR**

```bash
git push -u origin codex/browser-isolation
gh pr create --base main --head codex/browser-isolation --title "Browser Phase A: prove sandbox isolation" --body-file /tmp/plume-browser-isolation-pr.md
```

The PR description must list the exact head SHA, threat model, tests, packaged-smoke scope, and all non-goals. It must not claim a usable Browser workspace.

**Step 6: Commission exact-head independent review and stop before merge**

Ask the independent reviewer to inspect the exact pushed head and verify:

- application-manifest/handler/capability parity;
- main-webview-only authority;
- zero sandbox capability match;
- real MockRuntime denial behavior;
- URL/popup/download policy;
- stale lifecycle/window races;
- absence of page-to-Plume bridges;
- honest documentation and missing tests.

Do not merge until the exact-head review has no unresolved Critical/Important finding and GitHub CI plus secret scan are green.

## Plan self-review

- Every design section is covered: command registry, webview labels, lifecycle, URL policy, one-way observation, command-layer defense, runtime proof, docs, and honest smoke scope.
- Every production behavior begins with a failing focused test.
- The command set has one source of truth shared by build-time manifest and verification.
- The browser sandbox never appears in a capability selector.
- No placeholders, `TODO`, `TBD`, fake future slice number, or unimplemented evidence claim is included.
- The plan ends at an open reviewed PR; human Browser UI and typed evidence remain separate coherent PRs under the active goal.
