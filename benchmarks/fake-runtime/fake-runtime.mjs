#!/usr/bin/env node
// D129: deterministic fake local runtime for harness tests.
//
// This is NOT a model and never pretends to be one: records produced
// against it carry engine "plume-fake-runtime" / backend "scripted",
// which the summarizer banners as harness test data. Its whole job is
// to give the benchmark harness a runtime-shaped subprocess whose
// behavior — replies, tool calls, reported timings, malformed frames,
// crashes, hangs, cancellation — is scripted byte-for-byte by a case
// file, so harness mechanics can be tested with zero model downloads,
// zero inference, and zero network (stdio only, no ports).
//
// Protocol (line-delimited JSON):
//   stdin  → {"type":"generate","prompt":"..."}
//            {"type":"cancel"}
//   stdout ← {"type":"token","text":"..."}
//            {"type":"toolCall","tool":"...","args":{...}}
//            {"type":"done","report":{...scripted numbers...}}
//            {"type":"cancelled","report":{"cancellationLatencyMs":n}}
//
// Modes (case file "mode"): "complete" (default), "malformed" (emit a
// non-JSON line mid-stream), "crash" (exit 9 mid-stream), "hang"
// (first token, then silence — the harness timeout must fire),
// "cancellable" (stream until a cancel arrives, then acknowledge).
//
// `--health` prints {"type":"healthy"} and exits 0 — the restart
// probe. `--case <path>` selects the behavior script.

import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';

const args = process.argv.slice(2);

if (args.includes('--health')) {
  process.stdout.write(JSON.stringify({ type: 'healthy' }) + '\n');
  process.exit(0);
}

const caseFlag = args.indexOf('--case');
if (caseFlag === -1 || caseFlag + 1 >= args.length) {
  process.stderr.write('fake-runtime: --case <path> is required\n');
  process.exit(2);
}

let caseScript;
try {
  caseScript = JSON.parse(readFileSync(args[caseFlag + 1], 'utf8'));
} catch (err) {
  process.stderr.write(`fake-runtime: cannot read case file: ${err.message}\n`);
  process.exit(2);
}

// After a scripted crash, the harness restarts the runtime and sends
// a follow-up request; `--follow-up` selects the case's post-restart
// behavior (always a plain completion) instead of crashing again.
if (args.includes('--follow-up')) {
  caseScript = { ...(caseScript.followUp ?? {}), mode: 'complete' };
}

const mode = caseScript.mode ?? 'complete';
const emit = (event) => process.stdout.write(JSON.stringify(event) + '\n');

let cancelled = false;
let generating = false;

const rl = createInterface({ input: process.stdin, terminal: false });
rl.on('line', (line) => {
  let request;
  try {
    request = JSON.parse(line);
  } catch {
    process.stderr.write('fake-runtime: malformed request line\n');
    process.exit(2);
  }
  if (request.type === 'cancel') {
    cancelled = true;
    if (generating && mode === 'cancellable') {
      emit({
        type: 'cancelled',
        report: { cancellationLatencyMs: caseScript.report?.cancellationLatencyMs ?? 0 },
      });
      process.exit(0);
    }
    return;
  }
  if (request.type === 'generate' && !generating) {
    generating = true;
    generate();
  }
});

function generate() {
  for (const call of caseScript.toolCalls ?? []) {
    emit({ type: 'toolCall', tool: call.tool, args: call.args ?? {} });
  }

  const reply = caseScript.reply ?? '';
  // Emit the reply as word-ish tokens so the harness sees a stream.
  const tokens = reply.length > 0 ? reply.split(/(?<= )/) : [];

  if (mode === 'malformed') {
    if (tokens.length > 0) emit({ type: 'token', text: tokens[0] });
    process.stdout.write('%%% this is not a JSON frame %%%\n');
    process.exit(0);
  }
  if (mode === 'crash') {
    if (tokens.length > 0) emit({ type: 'token', text: tokens[0] });
    process.exit(9);
  }
  if (mode === 'hang') {
    if (tokens.length > 0) emit({ type: 'token', text: tokens[0] });
    // Never finish; ignore cancels. The harness timeout must fire.
    setInterval(() => {}, 1 << 30);
    return;
  }
  if (mode === 'cancellable') {
    // Emit the first tokens, then wait for the cancel that the
    // harness sends after observing a token.
    for (const token of tokens.slice(0, caseScript.tokensBeforeCancelWindow ?? 2)) {
      emit({ type: 'token', text: token });
    }
    setTimeout(() => {
      if (!cancelled) {
        // No cancel arrived: finish normally so a misconfigured test
        // fails loudly on the oracle instead of hanging.
        emit({ type: 'done', report: caseScript.report ?? {} });
        process.exit(0);
      }
    }, 2000);
    return;
  }

  for (const token of tokens) {
    emit({ type: 'token', text: token });
  }
  emit({ type: 'done', report: caseScript.report ?? {} });
  process.exit(0);
}
