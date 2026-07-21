# Release Proof

Evidence refreshed on July 21, 2026 from the final merged Build Week candidate.

## Candidate

- Integrated base: `origin/main@a0b62de4b603fe1ec3882addcc702d7d25bb1f5d`
- Artifact source head: `a0b62de4b603fe1ec3882addcc702d7d25bb1f5d`
- Product version: `0.1.0`
- License: `MIT`
- Bundle identifier: `dev.plume.app`
- Tauri bundle targets: `app`, `dmg`
- Architecture: Apple Silicon `arm64`
- Expected artifacts:
  - `src-tauri/target/release/bundle/macos/Plume.app`
  - `src-tauri/target/release/bundle/dmg/Plume_0.1.0_aarch64.dmg`
- Final DMG SHA-256:
  `73073a0d28ba208dc58546172bc5e48dbefb786db611f668da8d6288f0596291`

## Packaging commands

```bash
./scripts/dev-env.sh bash -lc \
  'source "$HOME/.cargo/env" 2>/dev/null; npm run tauri -- build --bundles app,dmg'
codesign --verify --deep --strict --verbose=2 \
  src-tauri/target/release/bundle/macos/Plume.app
file src-tauri/target/release/bundle/macos/Plume.app/Contents/MacOS/plume
shasum -a 256 src-tauri/target/release/bundle/dmg/Plume_0.1.0_aarch64.dmg
```

The final build completed from exact merged `main`. `hdiutil verify` reported
the DMG checksum as valid. The DMG was then mounted read-only and the bundled
`Plume.app` independently passed deep, strict `codesign` verification; its main
executable reported `arm64` and its bundle version reported `0.1.0`.

The artifact source head includes the squash-merged consumer UI, project
opening, shell cleanup, transcript-native research workflow, and post-merge
evidence repins. The final full verifier reported 53 pass, 3 documented
file-size warnings, and 0 fail. GitHub verify and gitleaks were also green on
the final code and post-merge cleanup pull requests.

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

The final app and DMG were rebuilt from exact merged commit `a0b62de`. Its
application tree matches the packaged transcript-native smoke at reviewed head
`90b4f5d`; the later squash merges and post-merge cleanup changed history,
evidence pointers, and a process-heavy test timeout without changing product
runtime behavior. The final DMG itself was freshly verified, mounted read-only,
and inspected as described above. Public upload remains a separate owner
action.
