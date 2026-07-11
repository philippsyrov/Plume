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
// The process is a SESSION: it stays alive after a completed or
// cancelled request and serves the next generate on the same stdin,
// which is what makes an honest warm population possible (runtime
// loaded + unrecorded priming request + measured requests, all in one
// process). Failure modes (malformed, crash, hang) end the process —
// that is the behavior they script. Every report carries
// `requestIndex` (0-based, per process) so tests can prove whether a
// request ran in a fresh or an already-loaded process.
//
// Protocol (line-delimited JSON):
//   stdin  → {"type":"generate","prompt":"..."}
//            {"type":"cancel"}
//   stdout ← {"type":"token","text":"..."}
//            {"type":"toolCall","tool":"...","args":{...}}
//            {"type":"done","report":{...scripted numbers...}}
//            {"type":"cancelled"}
//
// `--health` prints {"type":"healthy"} and exits 0 — the restart
// probe. `--case <path>` selects the behavior script. `--follow-up`
// selects the case's post-restart behavior (always a completion).

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

let requestIndex = -1;
let generating = false;
let cancelRequested = false;

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
    cancelRequested = true;
    if (generating && mode === 'cancellable') {
      generating = false;
      emit({ type: 'cancelled' });
    }
    return;
  }
  if (request.type === 'generate' && !generating) {
    generating = true;
    cancelRequested = false;
    requestIndex += 1;
    generate();
  }
});

function report() {
  return { ...(caseScript.report ?? {}), requestIndex };
}

function generate() {
  for (const call of caseScript.toolCalls ?? []) {
    emit({ type: 'toolCall', tool: call.tool, args: call.args ?? {} });
  }

  // `replyByRequest[i]` scripts a different reply for the i-th request
  // served by THIS process (0-based); requests past the array fall
  // back to `reply`. Tests use it to prove population honesty: a warm
  // measurement must arrive as request ≥ 1 of a primed process.
  const scripted = caseScript.replyByRequest?.[requestIndex];
  const reply = scripted !== undefined ? scripted : (caseScript.reply ?? '');
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
      if (!cancelRequested && generating) {
        // No cancel arrived: finish normally so a misconfigured test
        // fails loudly on the oracle instead of hanging.
        generating = false;
        emit({ type: 'done', report: report() });
      }
    }, 2000);
    return;
  }

  for (const token of tokens) {
    emit({ type: 'token', text: token });
  }
  generating = false;
  emit({ type: 'done', report: report() });
  // Session semantics: stay alive for the next generate.
}
