// D129: the harness's direct runtime client (`measurementPath:
// "rawRuntime"`). Speaks the line-delimited JSON protocol the fake
// runtime implements: spawn, send one generate request, collect the
// event stream to a terminal outcome, with fixture-timeout kill,
// deliberate cancellation, and post-crash restart/health/follow-up.
//
// Everything here is local subprocess I/O — no ports, no network.

import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';

export interface RuntimeReport {
  promptTokens?: number;
  outputTokens?: number;
  ttftMs?: number;
  promptEvaluationMs?: number;
  generationDurationMs?: number;
  endToEndMs?: number;
  acceptedContextTokens?: number;
  truncated?: boolean;
  cancellationLatencyMs?: number;
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
}

export interface InvocationOptions {
  command: string[];
  prompt: string;
  timeoutMs: number;
  /// Send {"type":"cancel"} after this many token events (deliberate
  /// cancellation for the cancellation-restart suite).
  cancelAfterTokens?: number;
}

function spawnRuntime(command: string[], extraArgs: string[] = []): ChildProcessWithoutNullStreams {
  const [bin, ...args] = command;
  if (bin === undefined) throw new Error('runtime command must not be empty');
  return spawn(bin, [...args, ...extraArgs], { stdio: ['pipe', 'pipe', 'pipe'] });
}

/// Run one invocation to a terminal outcome. Never throws on runtime
/// misbehavior — misbehavior IS the measurement; only harness-level
/// bugs (empty command) throw.
export function runInvocation(options: InvocationOptions, followUp = false): Promise<InvocationResult> {
  return new Promise((resolve) => {
    const child = spawnRuntime(options.command, followUp ? ['--follow-up'] : []);
    const toolCalls: RecordedToolCall[] = [];
    let reply = '';
    let tokensSeen = 0;
    let settled = false;
    let sawMalformed = false;
    let buffer = '';

    const timer = setTimeout(() => {
      settle({ terminal: 'timedOut', reply, toolCalls, report: null });
      child.kill('SIGKILL');
    }, options.timeoutMs);

    function settle(result: InvocationResult): void {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(result);
    }

    child.on('error', () => {
      settle({ terminal: 'crashed', reply, toolCalls, report: null });
    });

    child.on('exit', (code) => {
      // A terminal event already settled us; an exit without one is a
      // crash (unless we already flagged the stream malformed).
      if (!settled) {
        settle({ terminal: sawMalformed ? 'malformed' : 'crashed', reply, toolCalls, report: null });
      }
      void code;
    });

    child.stdout.on('data', (chunk: Buffer) => {
      buffer += chunk.toString('utf8');
      let newline = buffer.indexOf('\n');
      while (newline !== -1) {
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        handleLine(line);
        newline = buffer.indexOf('\n');
      }
    });

    function handleLine(line: string): void {
      if (settled || line.trim().length === 0) return;
      let event: unknown;
      try {
        event = JSON.parse(line);
      } catch {
        // Any non-JSON frame before a valid terminal event violates
        // the protocol: the stream is malformed.
        sawMalformed = true;
        settle({ terminal: 'malformed', reply, toolCalls, report: null });
        child.kill('SIGKILL');
        return;
      }
      const frame = event as { type?: unknown; text?: unknown; tool?: unknown; args?: unknown; report?: unknown };
      switch (frame.type) {
        case 'token': {
          if (typeof frame.text === 'string') reply += frame.text;
          tokensSeen += 1;
          if (options.cancelAfterTokens !== undefined && tokensSeen === options.cancelAfterTokens) {
            child.stdin.write(JSON.stringify({ type: 'cancel' }) + '\n');
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
          settle({ terminal: 'completed', reply, toolCalls, report: (frame.report as RuntimeReport) ?? {} });
          break;
        }
        case 'cancelled': {
          settle({ terminal: 'cancelled', reply, toolCalls, report: (frame.report as RuntimeReport) ?? {} });
          break;
        }
        default: {
          sawMalformed = true;
          settle({ terminal: 'malformed', reply, toolCalls, report: null });
          child.kill('SIGKILL');
        }
      }
    }

    child.stdin.write(JSON.stringify({ type: 'generate', prompt: options.prompt }) + '\n');
  });
}

/// Post-crash restart probe: spawn with --health and expect a healthy
/// frame before the timeout.
export function probeHealth(command: string[], timeoutMs: number): Promise<boolean> {
  return new Promise((resolve) => {
    const child = spawnRuntime(command, ['--health']);
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
