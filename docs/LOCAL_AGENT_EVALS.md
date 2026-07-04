# Local agent evals — model-quality notes

What the local models Plume targets are actually good and bad at
inside the patch-only single-step agent loop. These are **quality
notes, not benchmarks**: tiny probe counts, one machine, no scores.
The point is to record observed behavior honestly so slices that
touch the loop (prompting, UI copy, mode guidance) are grounded in
what the model really does — and so regressions in model behavior
after an `mlx-lm` or checkpoint upgrade have a before-picture.

Each dated section below is a snapshot. Re-running the probes after
a model / runtime change and appending a new section is the intended
way to extend this file — do not rewrite old sections.

## Qwen2.5-Coder-3B-Instruct-4bit (D125, 2026-07-04)

### Method

- **Model / runtime:** `Qwen2.5-Coder-3B-Instruct-4bit` (MLX
  4-bit checkpoint under the Plume model dir), `mlx_lm` 0.31.3,
  Apple Silicon, `temperature 0.0`, `max_tokens 512`.
- **Prompt fidelity:** every probe used the app's *exact*
  single-step steering — the system message from
  `commands::agent::build_propose_diff_messages`
  (`src-tauri/src/commands/agent.rs`) and, for attached-file
  probes, the verbatim D99 attach wrap from
  `prompts::assemble::wrap_with_attachment`
  (`Attached file (read-only context): …` +
  `----- FILE BEGIN/END -----`). So the replies below are what
  `agent.singleStep` really sees, not a smoke-script paraphrase.
- **Cycle fidelity:** every cycle row ran through
  `run_propose_diff_cycle` — the validate → apply → revert
  helper behind the D91 smoke
  (`patch::propose_diff_smoke_tests`) — against a throwaway
  fixture seeded with the same 2-line `greet.py` the D91 smoke
  uses, invoked via the `#[ignore]`d `qwen_propose_diff_smoke`
  entry point (the same one `scripts/smoke-qwen-propose-diff.sh`
  drives). That entry point asserts a full pass: it passed for
  rows 1, 2, and 5, and for the drifted-fixture row 6 it printed
  the recorded `ApplyFailed(PreImageMismatch)` outcome and then
  failed its pass assertion — the expected result for that row,
  not a harness error. Replies were classified with the rules of
  `agent::single_step::classify_action` (diff markers beat the
  `TOOL_REQUEST:` sentinel; otherwise prose = no action).
- **Reproduction:** `scripts/smoke-qwen-propose-diff.sh` packages
  the default probe end-to-end (its instruction is inlined in the
  user message rather than the app's system message — close, not
  identical). The other probes are one-off curl calls against a
  locally started `mlx_lm server` using the two verbatim prompt
  pieces named above; no eval harness was added and none should
  be — if a probe set ever needs automation, that's its own
  decision, not a default.

### Results

Six cycle runs from five prompts. Outcome vocabulary:
**full pass** = validated, applied, reverted cleanly;
**blocked** = `TOOL_REQUEST:` sentinel → the app's blocked
`toolFailed` event, no diff, nothing to apply.

| # | Probe (gist) | Attached file | Reply | Cycle outcome |
|---|---|---|---|---|
| 1 | one-line modify: return an f-string | yes | unified diff (49 tok) | **full pass** |
| 2 | small refactor: rename `greet` → `build_greeting` | yes | unified diff (49 tok) | **full pass** |
| 3 | no-diff question: "what does `greet(\"World\")` return?" | yes | `TOOL_REQUEST: PythonExecutor` | **blocked** |
| 4 | new-file request: create `test_greet.py` with a pytest test | yes | `TOOL_REQUEST: pytest` | **blocked** |
| 5 | blind edit (same modify as #1, no file content shown) | no | unified diff (44 tok) | **full pass** (pre-image guessed correctly) |
| 6 | probe #1's diff re-applied against a drifted fixture (no new model call) | — | — | **apply failed** (`PreImageMismatch`, disk untouched) |

### What Qwen 3B is good at

- **Obeying the diff-only steering.** Zero prose and zero code
  fences across all five replies. Small, minimal diffs (44–49
  completion tokens); no invented extra hunks, no drive-by edits
  beyond the instruction.
- **Simple in-place edits of an attached file.** The one-line
  modify and the function rename both validated, applied, and
  reverted through Plume's real patch path on the first try at
  temperature 0.
- **Honoring the `TOOL_REQUEST:` escape hatch.** When the ask
  wasn't expressible as an edit it emitted exactly one sentinel
  line, as the system message documents — the blocked path fires
  as designed, and nothing reaches disk.

### What it is bad (or just odd) at

- **Sloppy diff mechanics, tolerated downstream.** Headers came
  back without `a/`/`b/` prefixes (`--- greet.py`), and hunk
  headers routinely overcounted (`@@ -1,3 +1,3 @@` over a 2-line
  change); probes 1–2 also rewrote unchanged lines as −/+ pairs
  instead of using context lines. Plume's parser accepted all of
  it, and the applier verifies every pre-image line against disk
  (probe 6's failure message names the exact mismatching line),
  so the sloppiness costs nothing today — but a stricter parser
  would reject most of what this model emits.
- **Questions don't get answered — they get tool-requested.**
  Probe 3 didn't produce prose; the model asked for a
  (nonexistent) `PythonExecutor` tool. In propose-diff mode this
  surfaces as a blocked event plus the D123 "no applicable diff"
  note, which is honest but unhelpful. Questions belong in chat
  mode; the single-step panel is for edits.
- **Hallucinated tool names.** `PythonExecutor`, `pytest` —
  neither exists in any catalog shown to the model. The sentinel
  contract contains the blast radius (unknown names are blocked,
  never resolved), but don't expect the requested name to mean
  anything.
- **File creation is out of reach in practice.** Plume's
  validate/apply path fully supports create-diffs
  (`--- /dev/null`), but the model doesn't reach for that shape —
  asked for a new test file, it tool-requested instead (D97 saw
  the same with `TOOL_REQUEST: create-file`). If create-by-diff
  ever matters, the steering prompt would need to document the
  `/dev/null` form explicitly; today "create a file" simply isn't
  something this model does in the loop.
- **Blind edits look right and can still be wrong — the applier
  is the net.** Without the file attached (probe 5) the model
  fabricates the pre-image. Here it guessed a 2-line function
  correctly; on any real file it won't, and the failure mode is
  probe 6's: a well-formed diff that validates but fails apply
  with `PreImageMismatch`, writing nothing. Attach the file for
  results; the safety story holds either way.

### Implications for the loop (no changes made)

- The patch-only boundary and the pre-image check are doing real
  work: every bad outcome observed ends in "nothing written",
  and the one destructive-looking case (blind edit) is caught at
  apply time, not trusted at validate time.
- The single-step UX guidance writes itself from probes 3–4:
  propose-diff mode is an *edit* surface. The D123 no-diff /
  blocked copy is what a user sees when they treat it as chat.
- Candidate future slices this data motivates (candidates only,
  deliberately not started here): documenting create-diffs in
  the steering prompt, and a friendlier UI hint when a blocked
  `TOOL_REQUEST:` names a tool that doesn't exist.
