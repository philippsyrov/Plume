# Release Proof

Evidence recorded on July 17, 2026 from the post-merge MIT release branch.

## Candidate

- Integrated base: `origin/main@0c0361867f220386ae11a35a7aecb23d6b18ed68`
- Artifact source head: `800ad6a21b7615b21481a8984d8994c6448436fa`
- Product version: `0.1.0`
- License: `MIT`
- Bundle identifier: `dev.plume.app`
- Tauri bundle targets: `app`, `dmg`
- Architecture: Apple Silicon `arm64`
- Expected artifacts:
  - `src-tauri/target/release/bundle/macos/Plume.app`
  - `src-tauri/target/release/bundle/dmg/Plume_0.1.0_aarch64.dmg`
- Final DMG SHA-256:
  `e73adf092a92bab5bcd08a6488d1bab2461356993c444c80248185e647f850ef`

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
evidence-only documentation commit. It descends from the squash-merged Build
Week submission at `0c03618` and adds the owner-approved MIT license, matching
package metadata, a release-metadata regression test, and post-squash evidence
repins. The focused release-metadata suite passed 4 tests, followed by the full
verifier at 39 pass, 3 documented file-size warnings, and 0 fail.

## Signing status

The candidate uses Tauri's ad-hoc signing identity (`-`). Strict `codesign`
verification succeeds, which keeps an Apple Silicon download from appearing as
a structurally damaged app. It is **not** Developer ID signed and is **not**
notarized; Gatekeeper therefore requires the manual Privacy & Security approval
documented in [judge testing](judge-testing.md).

The owner confirmed that no paid Apple Developer account is available, so the
ad-hoc path and the explicit **Privacy & Security → Open Anyway** instructions
are the intended Build Week distribution path. No notarization is claimed.

## Packaged smoke evidence

The qualifying UI was previously smoked as a real packaged `.app` at durable
commit `2a3520e`, not as a browser preview. That smoke covered:

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

The final app and DMG were rebuilt from durable commit `800ad6a`. Relative to
the squash-merged application code, that source commit changes licensing,
release metadata tests, and evidence pointers only; it does not change the
smoked product UI or runtime behavior. The final DMG itself was freshly
verified, mounted read-only, and inspected as described above. This
evidence-only documentation commit does not enter the application bundle. Its
exact PR head is recorded in the review handoff before any upload is approved.
