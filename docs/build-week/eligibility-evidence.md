# Eligibility And New-Work Evidence

## Official window

The OpenAI Build Week submission window begins July 13, 2026 at 9:00 AM PDT
and closes July 21, 2026 at 5:00 PM PDT (July 22 at 02:00 in Rome). Existing
projects may enter, but only meaningful work added during the window is judged.
See the [official rules](https://openai.devpost.com/rules).

## Repository boundary

- Last commit before the cutoff:
  `5bcbf93dc2e948418b2360d1dd5a591f088243f5`
- First qualifying commit: `ca2954f`
- Current audited `origin/main` at campaign start:
  `dab11c0b4e15ffbe3c8acac7786f97b5fc608e8b`
- Qualifying commits present at campaign start: **18**
- Rebased candidate integration point:
  `015125e48c742ce1c236c655270f2f929ebef236`
- Qualifying commits present at the integration point: **20**

The existing editor, basic local/project chat, guarded diff/apply/revert path,
and early memory foundation predate the window. They are foundation, not the
new-work claim.

## Meaningful qualifying extension

The original 18 in-window commits add four connected parts of the judge path:

1. **Visible context:** explicit context shelf, drag-and-drop context, typed
   references, and exact preview/send/persistence behavior (`0cf9d57`,
   `ed5e4df`).
2. **Browser evidence:** human-controlled Browser workspace, page-text and
   screenshot evidence, ownership isolation, persistence, recovery, and
   lifecycle hardening (`c504243`, `2258890`, `dbf8702`, `56e53e0`,
   `eba44a8`, `c84f4b8`, `0cdea8c`, `e574392`).
3. **Usable knowledge workspace:** consumer workspace unification, Library,
   scope-aware memory use, and Handbook/consumer polish (`8b8197d`, `f4138ae`,
   `337f7de`).
4. **Release hardening:** MLX lifecycle cleanup/recovery and worktree-safe local
   dependencies (`4ee608b`, `dab11c0`).

The rebased candidate additionally includes bounded MLX SSE and Ollama NDJSON
stream frames plus the post-squash inventory repin (`59a4e51`, `015125e`).
Those changes prevent an unbounded unterminated provider frame from growing in
memory while preserving valid frames exactly at the one-megabyte boundary.

This submission campaign adds release metadata, packaged-app evidence,
judge-testing documentation, and a focused composer/context-shelf cleanup on
top of that qualifying feature set.

## Codex collaboration evidence

The qualifying commits and their PR timestamps are the durable repository
record. The Devpost submission also requires the `/feedback` session ID for the
Codex task where most core functionality was built. A likely historical core
task is `019f5bce-1864-7c20-a0a2-6f2cf46e46df`, but the project owner must run
`/feedback` in the actual task and use the returned ID; this document does not
fabricate compliance.

## Token-use measurement

On July 17, the local Codex task index reported **2,728,441,708 processed
tokens** across 16 direct Plume tasks whose source was Codex Desktop/VS Code or
CLI, including **1,985,585,626 tokens on Sol**. This is roughly 2.7 billion
direct-task tokens and 2.0 billion Sol tokens. Those tasks identify the model as
`gpt-5.6-sol`; the official challenge requires Codex with GPT-5.6.

This figure includes cached input and other processed context. It is not a bill,
does not equal unique authored text, and deliberately excludes raw subagent
fan-out totals that would multiply repeated context. The number is a provenance
hook, not proof of product quality; the working build and commit evidence remain
the submission proof.
