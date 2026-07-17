# Release Proof

Evidence recorded on July 17, 2026 from the exact Build Week submission branch.

## Candidate

- Integrated base: `origin/main@015125e48c742ce1c236c655270f2f929ebef236`
- Artifact source head: `2a3520ecec407006c4783cc9eaa0879d3235c981`
- Product version: `0.1.0`
- Bundle identifier: `dev.plume.app`
- Tauri bundle targets: `app`, `dmg`
- Architecture: Apple Silicon `arm64`
- Expected artifacts:
  - `src-tauri/target/release/bundle/macos/Plume.app`
  - `src-tauri/target/release/bundle/dmg/Plume_0.1.0_aarch64.dmg`
- Final DMG SHA-256:
  `de5efb8678f1503f99188f1f4c6ff6a7fa8ff61f16d8ab9f494c5ae3eeb2c4cc`

## Packaging commands

```bash
./scripts/dev-env.sh bash -lc \
  'source "$HOME/.cargo/env" 2>/dev/null; npm run tauri -- build --bundles app,dmg'
codesign --verify --deep --strict --verbose=2 \
  src-tauri/target/release/bundle/macos/Plume.app
file src-tauri/target/release/bundle/macos/Plume.app/Contents/MacOS/plume
shasum -a 256 src-tauri/target/release/bundle/dmg/Plume_0.1.0_aarch64.dmg
```

The final build completed after the accepted UI state. `hdiutil verify` reported
the DMG checksum as valid. The DMG was then mounted read-only and the bundled
`Plume.app` independently passed deep, strict `codesign` verification; its main
executable reported `arm64` and its bundle version reported `0.1.0`.

The artifact source head is a durable PR commit and an ancestor of this
evidence-only documentation commit. It includes the bounded MLX SSE and Ollama
NDJSON streaming-frame work from `59a4e51`. Four focused frame-cap tests passed
(oversized rejection and exact-boundary acceptance for both adapters). The
Build Week UI/release suite then passed 27 tests, followed by the full verifier
at 39 pass and 0 fail. The focused ModeToggle test exercised the stable
**Make changes** accessible name in both pressed and unpressed states.

## Signing status

The candidate uses Tauri's ad-hoc signing identity (`-`). Strict `codesign`
verification succeeds, which keeps an Apple Silicon download from appearing as
a structurally damaged app. It is **not** Developer ID signed and is **not**
notarized; Gatekeeper therefore requires the manual Privacy & Security approval
documented in [judge testing](judge-testing.md).

No valid Apple Developer signing identity or notarization credentials were
present in the build environment. Developer ID signing/notarization remains an
optional owner-controlled release upgrade.

## Packaged smoke evidence

The release app was launched as a real `.app`, not a browser preview. The smoke
covered:

- trusted-project reopen;
- readable file preview and explicit File context attachment;
- human-controlled Browser navigation to `https://example.com` and explicit
  page evidence attachment;
- existing project-memory display and explicit Library handoff;
- three-source File/Web/Memory context shelf;
- implicit normal chat and a model-gated change action that stays absent with
  no selected model;
- honest copy that distinguishes bounded ambient project memory/topics from
  explicit sources pinned exactly;
- quit, relaunch, project reopen, and selected-context restoration;
- wide Chat and narrow Browser-split layout states; and
- honest no-model behavior without a model download.

The app and DMG were rebuilt from durable commit `2a3520e`, after rebasing onto
the integrated streaming head. This evidence-only documentation commit does
not enter the application bundle. Its exact PR head is recorded in the review
handoff before the DMG is uploaded.
