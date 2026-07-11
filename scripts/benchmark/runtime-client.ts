// D129: the harness's direct runtime client (`measurementPath:
// "rawRuntime"`). Speaks the line-delimited JSON protocol the fake
// runtime implements.
//
// A `RuntimeSession` owns ONE runtime process and can serve several
// invocations on it — that is what makes a warm population honest:
// the runtime is genuinely loaded, the priming request and the
// measured requests share the process. A cold attempt uses a fresh
// session per invocation (`processRestart`).
//
// Cancellation latency is measured HERE, monotonically
// (performance.now()), from writing the cancel request to the
// terminal cancelled acknowledgement or conclusive stream close.
// Nothing a runtime reports about its own cancel latency is trusted
// or even read — the protocol's cancelled frame carries no report.
//
// Everything here is local subprocess I/O — no ports, no network.

import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { performance } from 'node:perf_hooks';

export interface RuntimeReport {
  promptTokens?: number;
  outputTokens?: number;
  ttftMs?: number;
  promptEvaluationMs?: number;
  generationDurationMs?: number;
  endToEndMs?: number;
  acceptedContextTokens?: number;
  truncated?: boolean;
  /// Per-process request counter from the fake runtime; lets tests
  /// prove whether a request ran in a fresh or already-loaded process.
  requestIndex?: number;
}

export interface RecordedToolCall {
  tool: string;
  args: Record<string, unknown>;
}

export type InvocationTerminal = 'completed' | 'malformed' | 'cancelled' | 'timedOut' | 'crashed';

export interface InvocationResult {
  terminal: InvocationTerminal;
  reply: string;
  toolCalls: RecordedToolCall[];
  report: RuntimeReport | null;
  /// Harness-measured (monotonic) cancel-send → terminal-acknowledged
  /// duration. Non-null only when this invocation was cancelled.
  cancellationLatencyMs: number | null;
}

export interface InvokeOptions {
  prompt: string;
  timeoutMs: number;
  /// Send {"type":"cancel"} after this many token events (deliberate
  /// cancellation for the cancellation-restart suite).
  cancelAfterTokens?: number;
}

export class RuntimeSession {
  private readonly child: ChildProcessWithoutNullStreams;
  private alive = true;
  private buffer = '';
  private lineHandler: ((line: string) => void) | null = null;
  private exitHandler: ((code: number | null) => void) | null = null;

  constructor(command: string[], extraArgs: string[] = []) {
    const [bin, ...args] = command;
    if (bin === undefined) throw new Error('runtime command must not be empty');
    this.child = spawn(bin, [...args, ...extraArgs], { stdio: ['pipe', 'pipe', 'pipe'] });
    this.child.on('error', () => {
      this.alive = false;
      this.exitHandler?.(null);
    });
    this.child.on('exit', (code) => {
      this.alive = false;
      this.exitHandler?.(code);
    });
    this.child.stdout.on('data', (chunk: Buffer) => {
      this.buffer += chunk.toString('utf8');
      let newline = this.buffer.indexOf('\n');
      while (newline !== -1) {
        const line = this.buffer.slice(0, newline);
        this.buffer = this.buffer.slice(newline + 1);
        this.lineHandler?.(line);
        newline = this.buffer.indexOf('\n');
      }
    });
  }

  get processAlive(): boolean {
    return this.alive;
  }

  close(): void {
    this.lineHandler = null;
    this.exitHandler = null;
    if (this.alive) this.child.kill('SIGKILL');
  }

  /// Run one invocation to a terminal outcome. Never throws on
  /// runtime misbehavior — misbehavior IS the measurement.
  invoke(options: InvokeOptions): Promise<InvocationResult> {
    return new Promise((resolve) => {
      const toolCalls: RecordedToolCall[] = [];
      let reply = '';
      let tokensSeen = 0;
      let settled = false;
      let sawMalformed = false;
      let cancelSentAt: number | null = null;

      if (!this.alive) {
        resolve({ terminal: 'crashed', reply, toolCalls, report: null, cancellationLatencyMs: null });
        return;
      }

      const timer = setTimeout(() => {
        settle({ terminal: 'timedOut', reply, toolCalls, report: null, cancellationLatencyMs: null });
        this.close();
      }, options.timeoutMs);

      const settle = (result: InvocationResult): void => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.lineHandler = null;
        this.exitHandler = null;
        resolve(result);
      };

      this.exitHandler = (code) => {
        if (settled) return;
        if (sawMalformed) {
          settle({ terminal: 'malformed', reply, toolCalls, report: null, cancellationLatencyMs: null });
        } else if (cancelSentAt !== null && code === 0) {
          // Cancel was requested and the stream conclusively closed
          // without a terminal frame: that closure IS the terminal
          // acknowledgement (contract: "or the stream is conclusively
          // closed").
          settle({
            terminal: 'cancelled',
            reply,
            toolCalls,
            report: null,
            cancellationLatencyMs: performance.now() - cancelSentAt,
          });
        } else {
          settle({ terminal: 'crashed', reply, toolCalls, report: null, cancellationLatencyMs: null });
        }
      };

      this.lineHandler = (line) => {
        if (settled || line.trim().length === 0) return;
        let event: unknown;
        try {
          event = JSON.parse(line);
        } catch {
          // Any non-JSON frame before a valid terminal event violates
          // the protocol: the stream is malformed.
          sawMalformed = true;
          settle({ terminal: 'malformed', reply, toolCalls, report: null, cancellationLatencyMs: null });
          this.close();
          return;
        }
        const frame = event as { type?: unknown; text?: unknown; tool?: unknown; args?: unknown; report?: unknown };
        switch (frame.type) {
          case 'token': {
            if (typeof frame.text === 'string') reply += frame.text;
            tokensSeen += 1;
            if (options.cancelAfterTokens !== undefined && tokensSeen === options.cancelAfterTokens) {
              // Latency clock starts when the harness ISSUES the
              // cancel — monotonic, client-side.
              cancelSentAt = performance.now();
              this.child.stdin.write(JSON.stringify({ type: 'cancel' }) + '\n');
            }
            break;
          }
          case 'toolCall': {
            if (typeof frame.tool === 'string') {
              const args =
                typeof frame.args === 'object' && frame.args !== null && !Array.isArray(frame.args)
                  ? (frame.args as Record<string, unknown>)
                  : {};
              toolCalls.push({ tool: frame.tool, args });
            }
            break;
          }
          case 'done': {
            settle({
              terminal: 'completed',
              reply,
              toolCalls,
              report: (frame.report as RuntimeReport) ?? {},
              cancellationLatencyMs: null,
            });
            break;
          }
          case 'cancelled': {
            settle({
              terminal: 'cancelled',
              reply,
              toolCalls,
              report: null,
              cancellationLatencyMs: cancelSentAt !== null ? performance.now() - cancelSentAt : null,
            });
            break;
          }
          default: {
            sawMalformed = true;
            settle({ terminal: 'malformed', reply, toolCalls, report: null, cancellationLatencyMs: null });
            this.close();
          }
        }
      };

      this.child.stdin.write(JSON.stringify({ type: 'generate', prompt: options.prompt }) + '\n');
    });
  }
}

/// One-shot convenience: fresh session, one invocation, close. This
/// is the COLD shape (`processRestart`) and the follow-up probe shape.
export async function runInvocation(
  command: string[],
  options: InvokeOptions,
  followUp = false,
): Promise<InvocationResult> {
  const session = new RuntimeSession(command, followUp ? ['--follow-up'] : []);
  try {
    return await session.invoke(options);
  } finally {
    session.close();
  }
}

/// Post-crash restart probe: spawn with --health and expect a healthy
/// frame before the timeout.
export function probeHealth(command: string[], timeoutMs: number): Promise<boolean> {
  return new Promise((resolve) => {
    const [bin, ...args] = command;
    if (bin === undefined) throw new Error('runtime command must not be empty');
    const child = spawn(bin, [...args, '--health'], { stdio: ['pipe', 'pipe', 'pipe'] });
    let output = '';
    let settled = false;
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        child.kill('SIGKILL');
        resolve(false);
      }
    }, timeoutMs);
    child.stdout.on('data', (chunk: Buffer) => {
      output += chunk.toString('utf8');
    });
    child.on('error', () => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        resolve(false);
      }
    });
    child.on('exit', (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        const frame: unknown = JSON.parse(output.trim().split('\n')[0] ?? '');
        resolve(code === 0 && typeof frame === 'object' && frame !== null && (frame as { type?: unknown }).type === 'healthy');
      } catch {
        resolve(false);
      }
    });
  });
}
