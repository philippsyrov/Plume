# Unified consumer shell design QA

## Comparison target

- Visual truth: `.superpowers/sdd/design-evidence/codex-browser-split-reference.png` and `.superpowers/sdd/design-evidence/codex-browser-expanded-reference.png`.
- Final packaged captures: `.superpowers/sdd/design-evidence/plume-browser-split-final.jpeg` and `.superpowers/sdd/design-evidence/plume-browser-expanded-final.jpeg`.
- Final implementation: `Plume Smoke.app` at branch head `5755177`, 1152 x 768, macOS dark appearance, casual task, public Google page, no running model.

The Codex references were captured in a larger window, so this comparison uses
region order, hierarchy, relative balance, control language, and task/browser
behavior rather than pretending the pixels are normalized.

## Final visual comparison

- Split view now keeps the task chat on the left and the task-owned Browser on the right, separated by one restrained persisted-width resizer.
- Browser chrome uses the shared icon family, one tab row, a conventional address row, quiet evidence attachment, and an explicit expand/return control.
- Expanded mode gives the page the main canvas and keeps a compact centered composer below it. The composer remains in reserved parent-WebView space rather than falsely overlaying the native child WebView.
- The consumer shell uses one opaque titlebar, one sidebar identity, one system UI font family, consistent control sizing, and no technical AGENTS/context scaffold in the primary empty state.
- The 988 px compact rule stacks Browser and chat instead of hiding the composer. The supported-minimum layout test pins the breakpoint and Browser geometry.

No P0, P1, or P2 visual issue remains in the compared Browser states.

## Packaged interaction proof

- A public Google page restored for the same task after rebuilding and relaunching the packaged app; Browser remained an explicit task workspace rather than a global browsing session.
- Split → expanded → split worked in the real macOS WebKit bundle and retained the task, tab, URL, and page.
- An unfinished address draft remained byte-for-byte visible after more than one 400 ms persistence poll.
- The Attach menu opened as an accessible menu. Moving focus directly into the native child WebView closed it, proving the parent/child focus fallback in the packaged app.
- Back, Forward, Reload, address, tab, new-tab, Attach, expand/return, resizer, and composer controls were all exposed through accessibility.
- Recovery and manual-reopen behavior is pinned across UI, hook, IPC, runtime, and persisted-store tests. Explicit reopen is user-driven, loopback still needs project scope plus exact-origin approval, and a successful matching reopen clears the persisted privacy gate.

## Intentional boundaries

- The native child WebView cannot safely share z-order with an HTML composer. Expanded mode therefore reserves a compact centered composer row below the page instead of drawing a fake overlay that WebKit could cover.
- Browser is a first-class workspace inside a task; Plume still opens the ordinary chat surface on fresh app launch. Reopening Browser restores that task's tabs, page, width, and layout.
- This is guarded Phase A browsing and local-web testing, not autonomous browsing or blanket macOS computer control.

## Verification record

- Browser/App focused frontend suite: 60 passed before the reopen fix; the final reopen review independently ran 48 Browser/API tests and found no Critical or Important issue.
- Persisted explicit-reopen regression and exact loopback approval regression passed in Rust.
- TypeScript typecheck, cargo formatting, commit verifier, and gitleaks passed at each final correction.
- Two independent exact-head reviews found no remaining Critical or Important Browser finding.

final result: passed
