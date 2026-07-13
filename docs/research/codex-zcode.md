```research-metadata
{
  "family": "codex-zcode",
  "sourceDate": "2026-07-13",
  "hygiene": "local-observation",
  "sources": ["https://openai.com/index/codex-for-almost-everything/", "https://zcode.z.ai/en"],
  "refreshTrigger": "Meaningful upstream product or public API release"
}
```

# Codex Desktop And ZCode

## Observed behavior

Official Codex material is the source for capability claims: parallel agents,
files and terminals, pull-request review, SSH, browser/computer-use work, and
repeatable scheduled tasks. ZCode is recorded only as a dated visual
observation on 2026-07-13: its visible task workspace resembles the Codex
desktop pattern. That observation does not establish its internal architecture
or implementation.

## Plume adaptation

Use one clear workspace with separately statused Files, Review, Terminal,
Browser, progress, diff, and background-task surfaces. Preserve the provenance
of every attached source and follow-up instead of turning placed context into
hidden authority. Plume remains a Tauri/Rust, local-first product.

## Already shipped overlap

Plume already ships the file workspace, safe diff/apply/revert lifecycle,
persisted chat sessions, and an externally operable UI. The Browser row and
optional browser tool descriptions are scaffolds only; Plume does not ship a
browser executor or computer-use emission.

## Remaining gap

The bounded multi-step agent loop, approved command execution, scheduled work,
visible subagent activity, sandboxed Browser workspace, and outbound
computer-use sessions remain unshipped.

## Rejected or deferred

Do not copy Codex or ZCode branding, product text, proprietary assets, or
Electron code. Do not infer architecture from pixels. Host computer control is
deferred until after the isolated browser sandbox and its safety evidence.
