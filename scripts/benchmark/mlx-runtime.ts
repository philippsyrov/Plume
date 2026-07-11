// D129A: real MLX-LM runtime adapter (`transport: "openai-sse"`).
//
// Owns one `python -m mlx_lm server --model <dir> --host 127.0.0.1
// --port <ephemeral>` process the same way Plume's supervisor does
// (spawn → poll GET /health → serve → SIGINT, then SIGKILL after a
// grace). One `MlxSession` = one loaded server process, so warm
// populations are honest exactly as with the fake runtime: priming
// and measured requests share the process; a cold attempt starts a
// fresh server (model load included in the attempt's world, not its
// timings — timings start at request send).
//
// Timing is CLIENT-OBSERVED (`timing.method: "clientObserved"`),
// monotonic via performance.now():
//   * timeToFirstTokenMs — request write → first non-empty content
//     delta ("a non-token status frame does not qualify").
//   * generationDurationMs — first content delta → terminal [DONE].
//   * endToEndMs — request write → terminal.
//   * promptEvaluationMs — NOT client-observable; always null here.
// Token counts come ONLY from the server's reported `usage` (we ask
// via stream_options.include_usage); counting SSE deltas is not
// authoritative and is never done.
//
// Everything talks to 127.0.0.1 only. Nothing here downloads models
// or installs packages; a missing interpreter, import, or checkpoint
// is a refusal with a diagnostic.

import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import net from 'node:net';
import { performance } from 'node:perf_hooks';

import type { InvocationResult, InvokeOptions, RuntimeReport } from './runtime-client.ts';
import type { SamplingBlock } from './types.ts';

export interface MlxServerConfig {
  /// Interpreter + args that start the server, WITHOUT host/port —
  /// the session appends `--host 127.0.0.1 --port <ephemeral>`.
  command: string[];
  /// Model directory the server loads; digested for identity
  /// verification by the caller before the session starts.
  modelDir: string;
  startupTimeoutMs?: number;
}

const DEFAULT_STARTUP_TIMEOUT_MS = 90_000;
const SHUTDOWN_GRACE_MS = 3_000;

function allocatePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.once('error', reject);
    probe.listen(0, '127.0.0.1', () => {
      const address = probe.address();
      if (address === null || typeof address === 'string') {
        probe.close(() => reject(new Error('could not allocate an ephemeral port')));
        return;
      }
      const port = address.port;
      probe.close(() => resolve(port));
    });
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/// Start the server and wait for /health. Rejects (with the last
/// stderr lines) if the process exits early or the budget runs out.
export async function startMlxSession(server: MlxServerConfig, sampling: SamplingBlock): Promise<MlxSession> {
  const [bin, ...args] = server.command;
  if (bin === undefined) throw new Error('mlx server command must not be empty');
  const port = await allocatePort();
  const child = spawn(bin, [...args, '--host', '127.0.0.1', '--port', String(port)], {
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  const stderrTail: string[] = [];
  const keepTail = (chunk: Buffer): void => {
    for (const line of chunk.toString('utf8').split('\n')) {
      if (line.trim().length === 0) continue;
      stderrTail.push(line);
      if (stderrTail.length > 20) stderrTail.shift();
    }
  };
  child.stdout.on('data', keepTail);
  child.stderr.on('data', keepTail);

  let exited = false;
  child.on('exit', () => {
    exited = true;
  });

  const budget = server.startupTimeoutMs ?? DEFAULT_STARTUP_TIMEOUT_MS;
  const deadline = performance.now() + budget;
  const base = `http://127.0.0.1:${port}`;
  for (;;) {
    if (exited) {
      throw new Error(`mlx server exited during startup. Last output:\n${stderrTail.join('\n')}`);
    }
    if (performance.now() > deadline) {
      child.kill('SIGKILL');
      throw new Error(`mlx server did not become healthy within ${budget} ms. Last output:\n${stderrTail.join('\n')}`);
    }
    try {
      const response = await fetch(`${base}/health`, { signal: AbortSignal.timeout(2_000) });
      if (response.ok) break;
    } catch {
      // Not up yet — keep polling.
    }
    await sleep(250);
  }
  return new MlxSession(child, base, sampling, server.modelDir);
}

export class MlxSession {
  private alive = true;

  constructor(
    private readonly child: ChildProcessWithoutNullStreams,
    private readonly base: string,
    private readonly sampling: SamplingBlock,
    private readonly modelId: string,
  ) {
    this.child.on('exit', () => {
      this.alive = false;
    });
  }

  get processAlive(): boolean {
    return this.alive;
  }

  async close(): Promise<void> {
    if (!this.alive) return;
    this.child.kill('SIGINT');
    const deadline = performance.now() + SHUTDOWN_GRACE_MS;
    while (this.alive && performance.now() < deadline) {
      await sleep(50);
    }
    if (this.alive) this.child.kill('SIGKILL');
  }

  /// One streamed chat completion, measured client-side. Never throws
  /// on runtime misbehavior — misbehavior IS the measurement.
  async invoke(options: InvokeOptions): Promise<InvocationResult> {
    const empty = (): Pick<InvocationResult, 'toolCalls' | 'report' | 'cancellationLatencyMs'> => ({
      toolCalls: [],
      report: null,
      cancellationLatencyMs: null,
    });
    if (!this.alive) return { terminal: 'crashed', reply: '', ...empty() };

    const body: Record<string, unknown> = {
      // MUST be the exact string passed to --model: mlx_lm.server
      // treats an unknown model id as something to RESOLVE (up to and
      // including a HuggingFace fetch) instead of using the loaded
      // model. The factory verifies command/--model/modelDir agree.
      model: this.modelId,
      stream: true,
      stream_options: { include_usage: true },
      messages: [{ role: 'user', content: options.prompt }],
      max_tokens: this.sampling.maxOutputTokens,
    };
    if (this.sampling.temperature !== null) body['temperature'] = this.sampling.temperature;
    if (this.sampling.topP !== null) body['top_p'] = this.sampling.topP;
    if (this.sampling.seed !== null) body['seed'] = this.sampling.seed;
    if (this.sampling.stopSequences.length > 0) body['stop'] = this.sampling.stopSequences;

    const controller = new AbortController();
    const sentAt = performance.now();
    let firstTokenAt: number | null = null;
    let cancelSentAt: number | null = null;
    let tokensSeen = 0;
    let reply = '';
    let usage: { prompt_tokens?: number; completion_tokens?: number } | null = null;
    let sawDone = false;
    let malformed = false;

    const timeoutTimer = setTimeout(() => controller.abort(), options.timeoutMs);
    const timedOutBy = (): boolean => performance.now() - sentAt >= options.timeoutMs;

    try {
      const response = await fetch(`${this.base}/v1/chat/completions`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
        signal: controller.signal,
      });
      if (!response.ok || response.body === null) {
        return { terminal: 'malformed', reply, ...empty() };
      }
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        let newline = buffer.indexOf('\n');
        while (newline !== -1) {
          const line = buffer.slice(0, newline).trim();
          buffer = buffer.slice(newline + 1);
          newline = buffer.indexOf('\n');
          if (line.length === 0 || !line.startsWith('data:')) continue;
          const payload = line.slice(5).trim();
          if (payload === '[DONE]') {
            sawDone = true;
            continue;
          }
          let frame: unknown;
          try {
            frame = JSON.parse(payload);
          } catch {
            malformed = true;
            controller.abort();
            break;
          }
          const chunk = frame as {
            choices?: Array<{ delta?: { content?: unknown } }>;
            usage?: { prompt_tokens?: number; completion_tokens?: number };
          };
          const content = chunk.choices?.[0]?.delta?.content;
          if (typeof content === 'string' && content.length > 0) {
            if (firstTokenAt === null) firstTokenAt = performance.now();
            reply += content;
            tokensSeen += 1;
            if (options.cancelAfterTokens !== undefined && tokensSeen === options.cancelAfterTokens) {
              // Deliberate cancel: for an HTTP stream the cancel IS
              // closing the request; latency runs until the stream is
              // conclusively closed below.
              cancelSentAt = performance.now();
              controller.abort();
            }
          }
          if (chunk.usage !== undefined && chunk.usage !== null) usage = chunk.usage;
        }
        if (malformed) break;
      }
    } catch {
      // Abort (cancel/timeout) or connection loss lands here; sorted
      // out below by cause.
    } finally {
      clearTimeout(timeoutTimer);
    }

    const closedAt = performance.now();
    if (malformed) return { terminal: 'malformed', reply, ...empty() };
    if (cancelSentAt !== null) {
      return {
        terminal: 'cancelled',
        reply,
        toolCalls: [],
        report: null,
        cancellationLatencyMs: closedAt - cancelSentAt,
      };
    }
    if (sawDone) {
      const report: RuntimeReport = {
        ...(typeof usage?.prompt_tokens === 'number' && typeof usage.completion_tokens === 'number'
          ? { promptTokens: usage.prompt_tokens, outputTokens: usage.completion_tokens }
          : {}),
        ...(firstTokenAt !== null ? { ttftMs: firstTokenAt - sentAt } : {}),
        ...(firstTokenAt !== null ? { generationDurationMs: closedAt - firstTokenAt } : {}),
        endToEndMs: closedAt - sentAt,
      };
      return { terminal: 'completed', reply, toolCalls: [], report, cancellationLatencyMs: null };
    }
    if (timedOutBy()) return { terminal: 'timedOut', reply, ...empty() };
    return { terminal: 'crashed', reply, ...empty() };
  }
}
