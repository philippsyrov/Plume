# Apple And Qwen Model Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Apple On-Device and a one-click Apache-licensed Qwen Coder model usable from Plume's top model selector without Ollama, external Python, or project trust.

**Architecture:** Rust owns an app-level fixed model catalog, verified Qwen download/install state, and the existing MLX supervisor. A bundled Swift helper bridges Apple's on-device Foundation Models API over bounded stdin/stdout JSON lines, while a relocatable bundled Python/MLX-LM payload serves the catalog Qwen checkpoint. React consumes typed catalog state and presents two compact model cards from one top-bar chooser.

**Tech Stack:** Tauri 2, Rust 2021, blocking `reqwest` with rustls, Swift 6 + FoundationModels, React 19, TypeScript, MLX-LM 0.31.3, MLX 0.32.0, mlx-metal 0.32.0, Vitest, Rust unit/integration tests, Swift Testing/XCTest, packaged macOS Computer Use smoke.

## Global Constraints

- Apple generation is on-device `SystemLanguageModel.default` only; never use Private Cloud Compute.
- Apple requires macOS 26 or newer and must stay visibly disabled with the backend-provided reason when unavailable.
- The fixed Qwen catalog entry is `mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit` at revision `b3252a2f97102b1fb1571fec2c9b27219a8536be`, reported size `868,628,559` bytes, Apache-2.0.
- Qwen downloads only after an explicit click and no model silently updates.
- Catalog download/start is app-level and works without a project. Existing arbitrary local-model starts keep the trusted-project gate.
- Rust owns paths, URLs, process launch, hashing, cancellation, prompt assembly, trust, redaction, and event sequencing. The frontend sends only catalog ids and opaque handles.
- Release builds use the bundled MLX runtime or fail honestly; they never silently execute an arbitrary `python` from `PATH`.
- Model weights live in Application Support, not the DMG. The Swift helper and MLX runtime live in the application bundle.
- Keep model links/backlinks unrelated to prompt authority; this slice adds no retrieval, broad tools, Browser authority, computer actions, or host control.
- Start every behavior change with a failing regression. Keep every code file at or below 800 lines.

---

### Task 1: App-Level Catalog State And Typed IPC

**Files:**
- Create: `src-tauri/src/providers/catalog.rs`
- Create: `src-tauri/src/providers/catalog_tests.rs`
- Create: `src-tauri/src/providers/catalog_manifest.json`
- Modify: `src-tauri/src/providers/mod.rs`
- Modify: `src-tauri/src/commands/providers.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app_commands.rs`
- Modify: `src/lib/api/providers.ts`
- Modify: `docs/IPC_CONTRACT.md`

**Interfaces:**
- Produces Rust `CatalogStore::new(app_data_dir: PathBuf)`, `CatalogStore::list() -> Vec<CatalogEntry>`, and `CatalogStore::qwen_install_dir() -> PathBuf`.
- Produces IPC `providers_catalog_list() -> Vec<CatalogEntry>`.
- Produces TypeScript `CatalogEntry`, `CatalogState`, and `listCatalogModels()`.
- Later tasks extend the same store with download operations and Apple availability; they do not invent a second catalog.

- [ ] **Step 1: Write failing catalog identity and path tests**

Add tests that pin the two ids, Qwen revision/license/size, app-data containment,
receipt-driven installed state, and rejection of a receipt whose catalog id,
revision, or manifest digest differs:

```rust
#[test]
fn catalog_is_fixed_and_qwen_install_lives_under_app_data() {
    let temp = tempfile::tempdir().unwrap();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let entries = store.list().unwrap();
    assert_eq!(entries.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(),
               ["apple-system", "qwen-coder-1.5b-mlx-4bit"]);
    let qwen = entries.iter().find(|entry| entry.id == QWEN_CATALOG_ID).unwrap();
    assert_eq!(qwen.revision.as_deref(), Some(QWEN_REVISION));
    assert_eq!(qwen.license, "Apache-2.0");
    assert!(store.qwen_install_dir().starts_with(temp.path()));
}

#[test]
fn mismatched_receipt_never_marks_qwen_installed() {
    let store = store_with_receipt(InstallReceipt {
        catalog_id: QWEN_CATALOG_ID.into(),
        revision: "wrong".into(),
        manifest_sha256: store.expected_manifest_sha256(),
        installed_bytes: QWEN_REPORTED_BYTES,
        completed_at_ms: 1,
    });
    assert_eq!(qwen_entry(&store).state, CatalogState::Absent);
}
```

- [ ] **Step 2: Run the focused Rust test and confirm RED**

Run: `./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test providers::catalog_tests -- --nocapture'`

Expected: FAIL because `CatalogStore`, constants, and types do not exist.

- [ ] **Step 3: Implement the fixed catalog and receipt validation**

Use a backend-owned enum and camelCase wire types:

```rust
pub const APPLE_CATALOG_ID: &str = "apple-system";
pub const QWEN_CATALOG_ID: &str = "qwen-coder-1.5b-mlx-4bit";
pub const QWEN_REVISION: &str = "b3252a2f97102b1fb1571fec2c9b27219a8536be";
pub const QWEN_REPORTED_BYTES: u64 = 868_628_559;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub display_name: String,
    pub subtitle: String,
    pub provider_id: String,
    pub model_id: String,
    pub state: CatalogState,
    pub availability_reason: Option<String>,
    pub download_bytes: Option<u64>,
    pub license: String,
    pub source_url: Option<String>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogState { Available, Unavailable, Absent, Installed, Running, Failed }
```

Resolve the model root as `<app_data>/models/catalog/<catalog-id>/<revision>`.
Read `install-receipt.json` with a 16 KiB cap, reject symlinks at every path
component, and treat parse/identity failure as `Absent`, never as installed.
Embed `catalog_manifest.json` with `include_bytes!` and compute its SHA-256 from
those exact bytes; the same digest is written into and checked from receipts.

- [ ] **Step 4: Register and wrap `providers_catalog_list`**

Manage one `Arc<CatalogStore>` in `AppState`, register the command in both
`generate_handler!` and `APP_COMMANDS`, and add:

```ts
export type CatalogState =
  | 'available' | 'unavailable' | 'absent' | 'installed' | 'running' | 'failed';

export function listCatalogModels(): Promise<CatalogEntry[]> {
  return invokeIpc<Record<string, never>, CatalogEntry[]>('providers_catalog_list', {});
}
```

- [ ] **Step 5: Run focused and contract tests**

Run:

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test providers::catalog_tests app_commands -- --nocapture'
npm run typecheck
npm run verify:docs
```

Expected: catalog tests PASS; command manifest stays exact; docs checker reports
only the two pre-existing Browser freshness notices.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/providers src-tauri/src/commands/providers.rs src-tauri/src/lib.rs src-tauri/src/app_commands.rs src/lib/api/providers.ts docs/IPC_CONTRACT.md
git commit -m "feat: add app-level model catalog"
```

### Task 2: Verified, Resumable Qwen Download Manager

**Files:**
- Create: `src-tauri/src/providers/catalog_download.rs`
- Create: `src-tauri/src/providers/catalog_download_tests.rs`
- Modify: `src-tauri/src/providers/catalog.rs`
- Modify: `src-tauri/src/providers/mod.rs`
- Modify: `src-tauri/src/commands/providers.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app_commands.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src/lib/api/providers.ts`
- Modify: `docs/IPC_CONTRACT.md`
- Modify: `docs/SAFETY.md`

**Interfaces:**
- Consumes `CatalogStore` and the fixed Qwen constants from Task 1.
- Produces `CatalogDownloadRegistry`, `begin_download`, `cancel_download`, `remove_catalog_model`, and event `providers/catalog-download`.
- Produces IPC wrappers `downloadCatalogModel`, `cancelCatalogDownload`, and `removeCatalogModel`.

- [ ] **Step 1: Verify and fill the immutable manifest**

Fetch Hugging Face metadata with blobs enabled at the pinned revision and
independently verify the checked-in required files, sizes, and SHA-256 values.
The nine small-file hashes below were computed from the pinned raw URLs; the
weight hash is the repository's pinned LFS SHA-256.

Run:

```bash
curl -L 'https://huggingface.co/api/models/mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit/revision/b3252a2f97102b1fb1571fec2c9b27219a8536be?blobs=true' -o /tmp/plume-qwen-manifest-source.json
```

The checked-in manifest contains exactly this data and no mutable URL:

```json
{
  "catalogId": "qwen-coder-1.5b-mlx-4bit",
  "repo": "mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit",
  "revision": "b3252a2f97102b1fb1571fec2c9b27219a8536be",
  "license": "Apache-2.0",
  "weightBytes": 868628559,
  "totalBytes": 880170581,
  "files": [
    { "path": "README.md", "size": 863, "sha256": "f164287c8bb0d595f172a4be0329660c35208249f360e0c84780e1fcc251e29b" },
    { "path": "added_tokens.json", "size": 605, "sha256": "58b54bbe36fc752f79a24a271ef66a0a0830054b4dfad94bde757d851968060b" },
    { "path": "config.json", "size": 785, "sha256": "0708e236aa7b7db3c01bf8a46200e1a92d57f20fa668a7c430154bd6a21cdba5" },
    { "path": "merges.txt", "size": 1671853, "sha256": "8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5" },
    { "path": "model.safetensors", "size": 868628559, "sha256": "daeab4764fb420d161721791cf2e509e2de81a7af4223646e7bed2bf82c57b58" },
    { "path": "model.safetensors.index.json", "size": 51569, "sha256": "6b98634d5044f0e2ad45228a374f8445904e571f1082392d08a6ce54f5d517ca" },
    { "path": "special_tokens_map.json", "size": 613, "sha256": "76862e765266b85aa9459767e33cbaf13970f327a0e88d1c65846c2ddd3a1ecd" },
    { "path": "tokenizer.json", "size": 7031673, "sha256": "a8506e7111b80c6d8635951a02eab0f4e1a8e4e5772da83846579e97b16f61bf" },
    { "path": "tokenizer_config.json", "size": 7228, "sha256": "dced901d401aa828f313a2baec70d0ce164653285812237b5285c7efd5b9e760" },
    { "path": "vocab.json", "size": 2776833, "sha256": "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910" }
  ]
}
```
The manifest parser rejects a zero size, repeated/unsafe path, non-64-character
hash, total mismatch, catalog mismatch, or revision mismatch.

- [ ] **Step 2: Write failing downloader tests using a fake fetcher**

Pin success, cancel, resume, byte overflow, hash mismatch, traversal, symlink,
redirect host policy, atomic rename, monotonic events, and removal refusal while
running. The fake fetcher returns bytes and redirect metadata without network:

```rust
#[test]
fn verified_download_installs_atomically_and_writes_receipt() {
    let fixture = DownloadFixture::matching_manifest();
    let result = fixture.manager.run(QWEN_CATALOG_ID, fixture.cancel.clone()).unwrap();
    assert_eq!(result.installed_bytes, fixture.expected_bytes());
    assert!(fixture.store.qwen_install_dir().join("install-receipt.json").is_file());
    assert!(!fixture.store.staging_dir().exists());
}

#[test]
fn one_byte_past_manifest_size_fails_without_installing() {
    let fixture = DownloadFixture::with_oversized_file(1);
    assert!(matches!(fixture.run(), Err(DownloadError::SizeMismatch { .. })));
    assert_eq!(qwen_entry(&fixture.store).state, CatalogState::Absent);
}
```

- [ ] **Step 3: Run the downloader test and confirm RED**

Run: `./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test providers::catalog_download_tests -- --nocapture'`

Expected: FAIL because the download manager is absent.

- [ ] **Step 4: Implement bounded download/install**

Add `reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }`
and `tempfile = "3"` to dev-dependencies. Use a custom redirect policy whose
pure predicate allows only `huggingface.co`, `cdn-lfs.huggingface.co`,
`cdn-lfs-us-1.huggingface.co`, `us.aws.cdn.hf.co`,
`cas-bridge.xethub.hf.co`, and `transfer.xethub.hf.co`. Cap redirects at 5 and
fail closed if Hugging Face changes delivery hosts until the checked-in policy
receives review.

Use `.part` files, `Range: bytes=<current>-`, `Content-Range` validation,
per-file exact size, total `manifest_total + 1 MiB` ceiling, streaming SHA-256,
and `fs::rename` on the same volume. The registry owns `Arc<AtomicBool>` cancel
flags and rejects a second active operation for the same catalog id.

Emit:

```rust
pub struct CatalogDownloadEvent {
    pub operation_id: String,
    pub seq: u64,
    pub catalog_id: String,
    pub phase: DownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}
```

- [ ] **Step 5: Add commands, frontend wrappers, and allowlist entries**

Register `providers_catalog_download`, `providers_catalog_download_cancel`, and
`providers_catalog_remove`. The download command returns `operationId`
immediately and emits progress from a bounded worker thread. Remove resolves
the fixed receipt-backed directory itself and refuses while the supervisor
reports a live catalog model id.

- [ ] **Step 6: Run focused suites**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test providers::catalog_download -- --nocapture'
npm run typecheck
npm run verify:docs
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/providers src-tauri/src/commands/providers.rs src-tauri/src/lib.rs src-tauri/src/app_commands.rs src-tauri/Cargo.toml src-tauri/Cargo.lock src/lib/api/providers.ts docs/IPC_CONTRACT.md docs/SAFETY.md
git commit -m "feat: download and verify catalog Qwen"
```

### Task 3: Bundled MLX Runtime Resolution And App-Level Qwen Start

**Files:**
- Create: `src-tauri/src/providers/mlx_runtime.rs`
- Create: `src-tauri/src/providers/mlx_runtime_tests.rs`
- Modify: `src-tauri/src/providers/mlx_lm/process.rs`
- Modify: `src-tauri/src/providers/mlx_lm/process_launch.rs`
- Modify: `src-tauri/src/providers/mlx_lm/process_tests.rs`
- Modify: `src-tauri/src/commands/providers.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app_commands.rs`
- Modify: `src/lib/api/providers.ts`
- Modify: `src/features/providers/useMlxServers.ts`
- Modify: `src/features/providers/useMlxServers.test.tsx`
- Modify: `docs/MLX_RUNTIME.md`
- Modify: `docs/IPC_CONTRACT.md`

**Interfaces:**
- Consumes receipt-verified `CatalogStore::installed_model_path(QWEN_CATALOG_ID)`.
- Produces `resolve_mlx_runtime(resource_dir, debug_build) -> Result<MlxCommand, RuntimeError>`.
- Produces `providers_catalog_start({catalogId}) -> ServerHandle`.
- Extends `MlxServersApi` with `startCatalog(catalogId)` while retaining generic `start(modelId)`.

- [ ] **Step 1: Write failing runtime resolution and trust-boundary tests**

```rust
#[test]
fn release_prefers_bundled_interpreter_and_never_path_python() {
    let bundle = fake_bundle_with_runtime();
    let command = resolve_mlx_runtime(bundle.path(), false).unwrap();
    assert_eq!(command.program, bundle.path().join("mlx-runtime/bin/python3"));
}

#[test]
fn release_without_bundle_fails_closed() {
    assert!(matches!(resolve_mlx_runtime(Path::new("/missing"), false),
                     Err(RuntimeError::BundledRuntimeMissing)));
}

#[test]
fn catalog_start_needs_no_project_but_generic_start_still_does() {
    let app = app_without_project_with_installed_qwen();
    assert!(catalog_start_for_test(&app, QWEN_CATALOG_ID).is_ok());
    assert!(matches!(generic_start_for_test(&app), Err(CommandError::NeedsApproval(_))));
}
```

- [ ] **Step 2: Run focused tests and confirm RED**

Run: `./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test mlx_runtime catalog_start -- --nocapture'`

- [ ] **Step 3: Refactor the supervisor to accept an explicit command**

Keep `start_server` as the generic entrypoint, and extract:

```rust
pub fn start_server_with_command(
    command: MlxCommand,
    model_path: &Path,
    inventory_model_id: &str,
) -> Result<ServerHandle, StartError> {
    try_start_once(command, model_path, inventory_model_id, StartupConfig::default())
}
```

Do not duplicate registry reservation, cap, spawn, health, commit, shutdown, or
diagnostics logic. Catalog and arbitrary local models must enter the same
supervisor below path resolution.

- [ ] **Step 4: Implement catalog start and frontend hook support**

`providers_catalog_start` accepts only `QWEN_CATALOG_ID`, revalidates its
receipt and non-symlink model path, resolves the packaged runtime from
`AppHandle::path().resource_dir()`, and calls `start_server_with_command`.
It never reads `ProjectSession` trust.

In `useMlxServers`, add a shared `startResolved(modelKey, starter)` helper so
generic and catalog starts share recovery, re-entry, unmount cleanup, and
status state. Add a regression proving catalog start works in no-project mode
while generic start still returns the existing approval message.

- [ ] **Step 5: Run focused suites**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test providers::mlx_runtime providers::mlx_lm commands::providers -- --nocapture'
npm test -- src/features/providers/useMlxServers.test.tsx
npm run typecheck
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/providers src-tauri/src/commands/providers.rs src-tauri/src/lib.rs src-tauri/src/app_commands.rs src/lib/api/providers.ts src/features/providers/useMlxServers.ts src/features/providers/useMlxServers.test.tsx docs/MLX_RUNTIME.md docs/IPC_CONTRACT.md
git commit -m "feat: start catalog Qwen without project trust"
```

### Task 4: Native Swift Foundation Models Helper

**Files:**
- Create: `src-tauri/apple-model/Package.swift`
- Create: `src-tauri/apple-model/Sources/PlumeAppleModel/Protocol.swift`
- Create: `src-tauri/apple-model/Sources/PlumeAppleModel/Availability.swift`
- Create: `src-tauri/apple-model/Sources/PlumeAppleModel/Generation.swift`
- Create: `src-tauri/apple-model/Sources/PlumeAppleModel/main.swift`
- Create: `src-tauri/apple-model/Tests/PlumeAppleModelTests/ProtocolTests.swift`
- Create: `src-tauri/apple-model/Tests/PlumeAppleModelTests/AvailabilityTests.swift`
- Create: `src-tauri/apple-model/Tests/PlumeAppleModelTests/GenerationTests.swift`
- Modify: `src-tauri/src/README.md`

**Interfaces:**
- Produces executable `plume-apple-model availability` and `plume-apple-model generate`.
- `availability` writes exactly one `AvailabilityResponse` JSON line.
- `generate` reads one `GenerationRequest` JSON object and writes bounded `token`, `done`, or `error` JSON lines.

- [ ] **Step 1: Write failing protocol and fake-session Swift tests**

Pin camelCase JSON, 1 MiB request refusal, availability reason mapping, delta
conversion from cumulative framework snapshots, and one terminal record:

```swift
func testCumulativeSnapshotsBecomeDeltas() async throws {
    let session = FakeSession(snapshots: ["APP", "APPLE", "APPLE OK"])
    let records = try await generate(request: .fixture, session: session)
    XCTAssertEqual(records.compactMap(\.delta), ["APP", "LE", " OK"])
    XCTAssertEqual(records.last?.kind, .done)
}

func testAppleIntelligenceDisabledIsAnUnavailableReason() {
    XCTAssertEqual(mapAvailability(.unavailable(.appleIntelligenceNotEnabled)).reason,
                   .appleIntelligenceDisabled)
}
```

- [ ] **Step 2: Run Swift tests and confirm RED**

Run: `swift test --package-path src-tauri/apple-model`

Expected: FAIL because the package implementation does not exist.

- [ ] **Step 3: Implement the helper protocol and availability**

Set `.macOS(.v26)` and produce one executable target plus one test target.
Use these wire shapes:

```swift
struct GenerationRequest: Codable {
    let requestId: String
    let messages: [ChatMessage]
    let maxOutputTokens: Int
}

enum OutputKind: String, Codable { case token, done, error }

struct OutputRecord: Codable {
    let kind: OutputKind
    let delta: String?
    let error: String?
}
```

Map system messages into `LanguageModelSession` instructions and format the
remaining bounded history with explicit `User:` / `Assistant:` labels. Use
`SystemLanguageModel.default` and `streamResponse(to:)`; turn cumulative
snapshots into suffix deltas. Never instantiate a PCC model.

- [ ] **Step 4: Implement CLI bounds and safe stderr**

Reject unknown modes, stdin over 1 MiB, more than 128 messages, a single
message over 256 KiB, output records over 1 MiB, and `maxOutputTokens` outside
`1...4096`. Emit safe error codes on stdout; reserve stderr for bounded
diagnostic text without prompt contents.

- [ ] **Step 5: Run Swift tests and compile release helper**

```bash
swift test --package-path src-tauri/apple-model
swift build -c release --package-path src-tauri/apple-model --product plume-apple-model
```

Expected: tests PASS and the helper is a thin `arm64` executable on the release builder.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/apple-model src-tauri/src/README.md
git commit -m "feat: add Apple Foundation Models helper"
```

### Task 5: Rust Apple Availability And Streaming Chat Adapter

**Files:**
- Create: `src-tauri/src/providers/apple_foundation.rs`
- Create: `src-tauri/src/providers/apple_foundation_tests.rs`
- Create: `src-tauri/src/chat/apple_foundation.rs`
- Create: `src-tauri/src/chat/apple_foundation_tests.rs`
- Modify: `src-tauri/src/providers/catalog.rs`
- Modify: `src-tauri/src/providers/mod.rs`
- Modify: `src-tauri/src/chat/mod.rs`
- Modify: `src-tauri/src/commands/providers.rs`
- Modify: `src-tauri/src/commands/chat/send_route.rs`
- Modify: `src-tauri/src/commands/chat/send.rs`
- Modify: `src-tauri/src/commands/chat/send_outcome.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app_commands.rs`
- Modify: `src/lib/api/providers.ts`
- Modify: `docs/IPC_CONTRACT.md`
- Modify: `docs/MODEL_PROVIDERS.md`

**Interfaces:**
- Consumes the Swift helper protocol from Task 4 and existing `chat/token`, `chat/done`, and cancel registry contracts.
- Produces `providers_apple_availability() -> AppleAvailability`.
- Adds chat route `providerId: "apple-foundation", modelId: "system"` with no server handle.

- [ ] **Step 1: Write failing parser, process, cancellation, and routing tests**

Use a `HelperPort` trait and fake implementation so ordinary Rust tests do not
depend on macOS 26. Pin nominal availability, every unavailable reason,
malformed/oversized lines, bounded stderr, one terminal event, cancellation,
deadline kill, and provider validation:

```rust
#[test]
fn cancelled_apple_stream_kills_helper_and_finishes_cancelled() {
    let helper = FakeHelper::hang_after_token("APP");
    let outcome = run_with_cancel(helper, Duration::from_millis(20));
    assert_eq!(outcome.finish, ChatFinish::Cancelled);
    assert!(outcome.helper_killed);
}

#[test]
fn apple_route_rejects_handle_and_non_system_model() {
    assert!(validate_apple_route("other", None).is_err());
    assert!(validate_apple_route("system", Some("handle")).is_err());
}
```

- [ ] **Step 2: Run focused Rust tests and confirm RED**

Run: `./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test apple_foundation -- --nocapture'`

- [ ] **Step 3: Implement helper resolution and availability**

Resolve `<resources>/apple-model/plume-apple-model`; on macOS below 26 return
`os-unsupported` before spawn. Read exactly one bounded line, require exit 0,
map typed helper reasons, and place only safe detail behind the catalog entry.
On non-macOS builds compile a stable `os-unsupported` implementation.

- [ ] **Step 4: Implement streaming without blocking cancellation**

Spawn the helper with piped stdin/stdout/stderr. A reader thread sends bounded
parsed records through `sync_channel(64)`. The routing thread uses
`recv_timeout(50ms)` to check cancel and the existing overall deadline; on
cancel/deadline it kills and waits for the child. Feed deltas into the existing
event emitter and produce the same `ChatDoneEvent` sequencing as MLX/Ollama.

- [ ] **Step 5: Register availability and route Apple chat**

Add `providers_apple_availability` to the command manifests. Extend chat
validation to accept exactly `apple-foundation/system`, reject `handleId`, and
dispatch after the same prompt assembly/redaction path used by other providers.
Do not silently fall back to Qwen on a failed Apple send.

- [ ] **Step 6: Run focused and full Rust suites**

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test apple_foundation commands::chat -- --nocapture'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test'
npm run typecheck
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/providers src-tauri/src/chat src-tauri/src/commands src-tauri/src/lib.rs src-tauri/src/app_commands.rs src/lib/api/providers.ts docs/IPC_CONTRACT.md docs/MODEL_PROVIDERS.md
git commit -m "feat: stream chat through Apple on-device model"
```

### Task 6: Frontend Catalog State Machine

**Files:**
- Create: `src/features/model-picker/useModelCatalog.ts`
- Create: `src/features/model-picker/useModelCatalog.test.tsx`
- Modify: `src/lib/api/providers.ts`
- Modify: `src/App.tsx`
- Modify: `src/features/project-shell/NoProjectChatView.tsx`
- Modify: `src/features/project-shell/UnifiedChrome.tsx`
- Modify: `src/features/model-picker/useSelectedModel.ts`
- Modify: `src/features/providers/useMlxServers.ts`

**Interfaces:**
- Consumes catalog IPC/download events, Apple availability, and `MlxServersApi.startCatalog`.
- Produces `ModelCatalogApi` with `entries`, `download`, `cancelDownload`, `useApple`, `useQwen`, `removeQwen`, and `refresh`.
- Hoists `SelectedModelApi` to `App` so one window keeps the same model while switching local/project views.

- [ ] **Step 1: Write failing state-machine regressions**

Pin initial listing, StrictMode single-list semantics, late-event rejection,
monotonic sequence handling, cancel/retry, Apple availability refresh, Qwen
start-before-select, start failure, and selection surviving project transitions:

```tsx
it('ignores late events from a cancelled download generation', async () => {
  const { result } = renderHook(() => useModelCatalog(deps));
  await act(() => result.current.download(QWEN_ID));
  act(() => emit({ operationId: 'old', seq: 2, phase: 'cancelled' }));
  await act(() => result.current.download(QWEN_ID));
  act(() => emit({ operationId: 'old', seq: 3, phase: 'complete' }));
  expect(result.current.entry(QWEN_ID).state).toBe('downloading');
});

it('selects Qwen only after catalog start returns an exact handle', async () => {
  startCatalog.mockResolvedValue({ id: 'h1', port: 62000, pid: 99 });
  await act(() => result.current.useQwen());
  expect(mlxServers.handleOf(QWEN_ID)).toEqual({ id: 'h1', port: 62000, pid: 99 });
  expect(select).toHaveBeenCalledWith(expect.objectContaining({ modelId: QWEN_ID }));
});
```

- [ ] **Step 2: Run frontend test and confirm RED**

Run: `npm test -- src/features/model-picker/useModelCatalog.test.tsx`

- [ ] **Step 3: Implement the hook and app-scoped ownership**

Subscribe once to `providers/catalog-download`, key progress by operation id,
reject non-monotonic `seq`, and refresh authoritative catalog state at terminal
events. `useApple` refreshes availability then selects only `available`.
`useQwen` awaits `startCatalog`, then selects:

```ts
{
  providerId: 'mlx-lm',
  providerDisplayName: 'Qwen Coder',
  modelId: QWEN_CATALOG_ID,
}
```

Do not copy the handle into `SelectedModel`: `useMlxServers` remains the single
live-handle owner, and ChatPanel already resolves `handleOf(modelId)` immediately
before each send. Move `useSelectedModel()` into `App` and pass the API to both
`TrustedView` and `NoProjectChatView`.

- [ ] **Step 4: Run focused tests and typecheck**

```bash
npm test -- src/features/model-picker/useModelCatalog.test.tsx src/features/providers/useMlxServers.test.tsx
npm run typecheck
```

- [ ] **Step 5: Commit**

```bash
git add src/lib/api/providers.ts src/App.tsx src/features/model-picker src/features/project-shell src/features/providers/useMlxServers.ts
git commit -m "feat: manage catalog models at window scope"
```

### Task 7: Beautiful Top-Bar Model Chooser And Empty-Chat Entry

**Files:**
- Create: `src/features/model-picker/ModelChooser.tsx`
- Create: `src/features/model-picker/ModelChooser.test.tsx`
- Create: `src/styles/layout/model-chooser.css`
- Modify: `src/features/project-shell/UnifiedChrome.tsx`
- Modify: `src/features/project-shell/NoProjectChatView.tsx`
- Modify: `src/features/chat/ChatPanel.tsx`
- Modify: `src/features/chat/ChatPanel.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/styles/index.css`
- Modify: `src/styles/layout/project-shell.css`
- Modify: `src/styles/layout/no-project.css`
- Modify: `src/features/README.md`

**Interfaces:**
- Consumes `ModelCatalogApi`, `SelectedModelApi`, and the existing Browser overlay-safety coordination.
- Produces controlled `ModelChooser({open, onOpenChange, catalog, selection})`.
- Produces `ChatPanel.onChooseModel` used only when no model is selected.

- [ ] **Step 1: Write failing chooser accessibility and flow tests**

Pin stable accessible name `Model`, value text, two card headings, terse Apple
reason, Download/progressbar/Cancel/Retry, Use actions, hidden technical detail,
keyboard close, outside click, focus return, and no paths/ports in the primary
surface:

```tsx
it('keeps a stable Model name while exposing the selected value', async () => {
  renderChooser({ selected: null });
  const trigger = screen.getByRole('button', { name: 'Model' });
  expect(trigger).toHaveTextContent('Choose model');
  await userEvent.click(trigger);
  expect(screen.getByRole('heading', { name: 'Apple On-Device' })).toBeVisible();
  expect(screen.getByRole('heading', { name: 'Qwen Coder 1.5B' })).toBeVisible();
});

it('shows an accessible download progressbar without technical clutter', () => {
  renderChooser({ qwen: { state: 'downloading', downloadedBytes: 100, totalBytes: 1000 } });
  expect(screen.getByRole('progressbar', { name: 'Downloading Qwen Coder' }))
    .toHaveAttribute('aria-valuenow', '10');
  expect(screen.queryByText(/\/Users\//)).toBeNull();
  expect(screen.queryByText(/port|pid/i)).toBeNull();
});
```

- [ ] **Step 2: Run focused UI tests and confirm RED**

Run: `npm test -- src/features/model-picker/ModelChooser.test.tsx src/features/chat/ChatPanel.test.tsx`

- [ ] **Step 3: Implement the controlled chooser**

Replace `NoProjectModelPicker` in `UnifiedTopBar` with one button and anchored
popover. Use two spacious cards, one primary action each, a `<progress>` or
ARIA progressbar, and `<details>` for source/license/error details. Keep copy
exactly as the design spec specifies.

Do not create a second Settings onboarding flow. Settings may retain advanced
provider inventory and add managed-model storage/removal under its existing
details surfaces.

- [ ] **Step 4: Integrate Browser overlay safety**

Add `modelChooserOpen` to both project and no-project shell state. Include it
in `htmlOverlayOpen`, pass a controlled open callback through `UnifiedTopBar`,
and render the popover only after `htmlOverlayReady`. A Browser native child
must suspend before the chooser appears, matching Settings/Help/Workspace
overlay behavior.

- [ ] **Step 5: Add the empty-chat entrypoint**

When `selected === null`, ChatPanel's empty state shows one **Choose a model**
button that invokes the same controlled chooser. The disabled composer keeps
the short placeholder `Choose a model to start chatting.` and no Settings
instruction.

- [ ] **Step 6: Run focused frontend suite and typecheck**

```bash
npm test -- src/features/model-picker src/features/chat/ChatPanel.test.tsx src/features/project-shell
npm run typecheck
```

- [ ] **Step 7: Commit**

```bash
git add src/features/model-picker src/features/project-shell src/features/chat src/styles src/App.tsx src/features/README.md
git commit -m "feat: choose and start models from the top bar"
```

### Task 8: Reproducible Runtime And Helper Packaging

**Files:**
- Create: `scripts/mlx-runtime-requirements.in`
- Create: `scripts/mlx-runtime-requirements.lock`
- Create: `scripts/build-mlx-runtime.sh`
- Create: `scripts/build-apple-model-helper.sh`
- Create: `scripts/prepare-model-runtime-bundle.sh`
- Create: `scripts/model-runtime-packaging.test.ts`
- Create: `src-tauri/runtime/README.md`
- Create: `src-tauri/third-party/NOTICE.md`
- Modify: `.gitignore`
- Modify: `package.json`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `scripts/smoke-app.sh`
- Modify: `scripts/verify.sh`
- Modify: `docs/DEPENDENCY_ISOLATION.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/SMOKE_TESTING.md`

**Interfaces:**
- Produces generated bundle resources `src-tauri/runtime/generated/mlx-runtime/**` and `src-tauri/runtime/generated/apple-model/plume-apple-model`.
- Produces `npm run prepare:model-runtime` and packaging verification.
- Consumed by Task 3 runtime resolution and Task 5 helper resolution.

- [ ] **Step 1: Write failing packaging tests**

Pin that release config includes both generated resources, the runtime lock
pins `mlx-lm==0.31.3`, `mlx==0.32.0`, and `mlx-metal==0.32.0`, generated
payloads are gitignored, notices exist, normal app builds cannot accidentally
embed model weights, and smoke-app prepares resources before bundling.

Run: `npm test -- scripts/model-runtime-packaging.test.ts`

Expected: FAIL because scripts/resources do not exist.

- [ ] **Step 2: Add deterministic runtime inputs**

`mlx-runtime-requirements.in` contains exactly:

```text
mlx-lm==0.31.3
mlx==0.32.0
mlx-metal==0.32.0
```

Generate the hashed lock through the project-local environment:

```bash
./scripts/dev-env.sh uv pip compile scripts/mlx-runtime-requirements.in --generate-hashes --python-version 3.12 -o scripts/mlx-runtime-requirements.lock
```

`build-mlx-runtime.sh` requires `uv`, installs pinned standalone CPython 3.12
into the generated resource root, syncs the hashed lock, strips caches/tests,
and runs `python3 -c 'import mlx, mlx_lm; print(mlx.__version__)'`. It writes a
runtime identity JSON containing Python/package versions and the lock digest.

- [ ] **Step 3: Build and stage the Swift helper**

`build-apple-model-helper.sh` runs Swift release build, copies only the helper
binary, verifies `file` reports arm64, and writes no source into the bundle.
`prepare-model-runtime-bundle.sh` invokes both builders and fails if either
identity check fails.

- [ ] **Step 4: Wire Tauri resources and release scripts**

Add both generated directories and `src-tauri/third-party/NOTICE.md` to
`bundle.resources`. Add `prepare:model-runtime` to `package.json`. Keep normal
frontend tests network-free; only packaging/smoke commands build the runtime.
Update `smoke-app.sh` to prepare resources before `tauri build`.

- [ ] **Step 5: Run packaging tests and build the payload**

```bash
npm test -- scripts/model-runtime-packaging.test.ts
npm run prepare:model-runtime
file src-tauri/runtime/generated/apple-model/plume-apple-model
src-tauri/runtime/generated/mlx-runtime/bin/python3 -c 'import mlx, mlx_lm; print(mlx.__version__, mlx_lm.__version__)'
```

Expected: arm64 helper; runtime prints pinned versions; no model weights exist
under `src-tauri/runtime/generated`.

- [ ] **Step 6: Commit**

```bash
git add scripts src-tauri/runtime/README.md src-tauri/third-party/NOTICE.md .gitignore package.json src-tauri/tauri.conf.json docs/DEPENDENCY_ISOLATION.md docs/DEVELOPMENT.md docs/SMOKE_TESTING.md
git commit -m "build: bundle Apple and MLX model runtimes"
```

### Task 9: Docs, Full Verification, Packaged Smoke, And Review Handoff

**Files:**
- Modify: `README.md`
- Modify: `docs/README.md`
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/MODEL_PROVIDERS.md`
- Modify: `docs/MLX_RUNTIME.md`
- Modify: `docs/IPC_CONTRACT.md`
- Modify: `docs/SAFETY.md`
- Modify: `docs/USER_GUIDE.md`
- Modify: `docs/DECOMPOSITION.md`
- Modify: `docs/build-week/judge-testing.md`
- Modify: `src/features/README.md`
- Modify: `src-tauri/src/README.md`

**Interfaces:**
- Records exact implementation ownership, current behavior, candidate limitations, runtime/model provenance, and judge path.
- Produces a ready PR only after exact-head local, packaged, CI, and opposing-review evidence.

- [ ] **Step 1: Update current truth without overclaiming**

Record Apple adapter shipped separately from host availability; MLX runtime
bundled separately from Qwen weights downloaded; Qwen catalog chat shipped
separately from deeper agent execution. Keep PCC, arbitrary downloads,
semantic retrieval, broad tools, agent Browser authority, and computer-use
emission explicitly unshipped.

Refresh only feature-inventory records whose owned paths changed in this PR.
Do not blindly stamp the pre-existing unrelated Browser freshness notices.

- [ ] **Step 2: Run focused suites**

```bash
swift test --package-path src-tauri/apple-model
npm test -- src/features/model-picker src/features/providers/useMlxServers.test.tsx src/features/chat/ChatPanel.test.tsx scripts/model-runtime-packaging.test.ts
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test providers::catalog providers::catalog_download providers::mlx_runtime apple_foundation commands::chat'
npm run typecheck
npm run verify:docs
```

- [ ] **Step 3: Run the full repository gate**

```bash
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
git diff --check
git status --short
```

Expected: zero failures; only documented existing doc soft-cap warnings and
honestly unresolved unrelated inventory notices.

- [ ] **Step 4: Build and inspect the exact-head app/DMG**

```bash
npm run prepare:model-runtime
./scripts/dev-env.sh npm run tauri -- build --bundles app,dmg
codesign --verify --deep --strict src-tauri/target/release/bundle/macos/Plume.app
file src-tauri/target/release/bundle/macos/Plume.app/Contents/MacOS/plume
hdiutil verify src-tauri/target/release/bundle/dmg/Plume_0.1.0_aarch64.dmg
shasum -a 256 src-tauri/target/release/bundle/dmg/Plume_0.1.0_aarch64.dmg
```

Verify the helper/runtime/notices exist inside the app and no Qwen weights are
inside the DMG.

- [ ] **Step 5: Run packaged Computer Use smoke at the exact head**

From Finder/Dock with no shell environment and Ollama stopped:

1. Both cards appear before a project is opened.
2. Apple exposes the truthful host state. If available, send/cancel a real
   reply; if the macOS beta still returns ModelManager error 1008, record it as
   an Apple framework blocker rather than a Plume pass.
3. Qwen Download → Cancel → Resume → Verify → Use works from the selector.
4. Qwen answers local chat, quits cleanly, relaunches, remains installed, and
   starts without another download.
5. Project context remains unavailable until a project is trusted.
6. Settings, Help, Workspace views, Browser overlay suspension/remount, and
   quit/relaunch remain operable.

- [ ] **Step 6: Commit docs and exact-head evidence**

```bash
git add README.md docs src/features/README.md src-tauri/src/README.md
git commit -m "docs: record Apple and Qwen onboarding evidence"
```

- [ ] **Step 7: Push ready PR and wait for gates**

```bash
git push -u origin codex/apple-qwen-model-onboarding
gh pr create --base main --head codex/apple-qwen-model-onboarding --title "Add first-run Apple and Qwen model onboarding" --body-file /tmp/plume-model-onboarding-pr.md
gh pr checks --watch
```

Require GitHub verify and gitleaks green at the exact pushed head.

- [ ] **Step 8: Exact-head findings-only review**

Review the live PR head against base, inspect enough surrounding code, and
verify every claim. Fix genuine Important findings with TDD, rerun the full
gate and packaged smoke at the new head, then request exact-head re-review.
Stop for external review; do not merge without the user's explicit instruction.
