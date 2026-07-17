# OpenAI Build Week 2026

This folder is the judge-facing source of truth for Plume's Build Week entry.
It separates the existing product foundation from work added during the
official July 13-21 submission window and keeps test claims tied to evidence.

- [Judge testing](judge-testing.md) — the shortest no-rebuild path through the
  packaged macOS app.
- [Release proof](release-proof.md) — platform, packaging, signing, and smoke
  evidence.
- [Eligibility evidence](eligibility-evidence.md) — qualifying-window commits,
  Codex collaboration evidence, and old-versus-new boundaries.
- [Demo script](demo-script.md) — a timed public-video storyboard under three
  minutes.
- [UI audit](audit/README.md) — the composer/context-shelf issue found during
  packaged-app testing and the accepted states.

## Submission position

**Category:** Developer Tools

**One sentence:** Plume is a local-first AI workspace that makes agent context,
browser evidence, memory, and file changes visible, inspectable, and
reversible.

The judge build supports **macOS on Apple Silicon**. It is not currently signed
with an Apple Developer ID or notarized. Do not describe the build as supporting
Intel Macs, Windows, or Linux.

The repository is licensed under MIT. The owner confirmed that no paid Apple
Developer account is available, so the judge build intentionally uses ad-hoc
signing and the documented **Privacy & Security → Open Anyway** path.

## Remaining owner actions

These steps require the project owner's explicit approval or account access and
are not performed by an automated coding task:

1. Upload the final DMG to a stable public download location.
2. Record and publish the public YouTube demo with audible narration.
3. Run `/feedback` in the Codex task that contains most of the qualifying core
   work and copy the returned session ID into the Devpost form.
4. Submit the project on Devpost.

Official challenge and rules:
[openai.devpost.com](https://openai.devpost.com/) and
[openai.devpost.com/rules](https://openai.devpost.com/rules).
