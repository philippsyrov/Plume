// @vitest-environment node
//
// D129A: MLX adapter tests against a deterministic LOCAL fake
// OpenAI-SSE server — a node http server on 127.0.0.1 with scripted
// responses. No model, no mlx-lm, no external network: this pins the
// adapter's protocol handling, client-observed timing, cancellation,
// and failure classification without the real runtime. The real
// mlx_lm.server path is exercised by scripts/benchmark-mlx-smoke.sh
// (needs mlx-lm + a local checkpoint; never part of npm run test).
//
// Node environment (not happy-dom): the adapter uses Node's real
// fetch/streams, and happy-dom's fetch does not speak them.

import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import type { Server } from 'node:http';
import { chmodSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';

import { digestModelDir } from './model-identity.ts';
import { MlxSession, startMlxSession } from './mlx-runtime.ts';
import type { SamplingBlock } from './types.ts';

const SAMPLING: SamplingBlock = {
  temperature: 0.0,
  topP: 1.0,
  topK: null,
  minP: null,
  repeatPenalty: 1.0,
  seed: 42,
  maxOutputTokens: 64,
  stopSequences: [],
};

interface ScriptedResponse {
  /// SSE frames to write, in order, with per-frame delay in ms.
  frames: Array<{ payload: string; delayMs?: number }>;
  /// End the response after the frames (default true).
  end?: boolean;
  /// Destroy the socket mid-stream instead of ending cleanly.
  destroy?: boolean;
  status?: number;
}

interface FakeServer {
  session: MlxSession;
  server: Server;
  requests: Array<Record<string, unknown>>;
  close(): Promise<void>;
}

/// A scripted OpenAI-SSE endpoint. Each POST /v1/chat/completions
/// consumes the next scripted response.
function startFakeSse(responses: ScriptedResponse[]): Promise<FakeServer> {
  const requests: Array<Record<string, unknown>> = [];
  let index = 0;
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (chunk: Buffer) => {
      body += chunk.toString('utf8');
    });
    req.on('end', () => {
      requests.push(JSON.parse(body) as Record<string, unknown>);
      const script = responses[Math.min(index, responses.length - 1)];
      index += 1;
      if (script === undefined) {
        res.writeHead(500).end();
        return;
      }
      res.writeHead(script.status ?? 200, { 'content-type': 'text/event-stream' });
      let delay = 0;
      for (const frame of script.frames) {
        delay += frame.delayMs ?? 0;
        setTimeout(() => res.write(`data: ${frame.payload}\n\n`), delay);
      }
      setTimeout(() => {
        if (script.destroy === true) res.destroy();
        else if (script.end !== false) res.end();
      }, delay + 5);
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (address === null || typeof address === 'string') throw new Error('no address');
      // A session over a server the test owns: hand it a dead child
      // stand-in via a tiny spawn of `node -e` that just sleeps, so
      // close() has something to kill.
      resolve({
        session: sessionFor(`http://127.0.0.1:${address.port}`),
        server,
        requests,
        close: () =>
          new Promise((done) => {
            server.close(() => done());
          }),
      });
    });
  });
}

/// Build an MlxSession pointed at an arbitrary base URL with an inert
/// child process (the fake server's lifecycle is the test's problem).
function sessionFor(base: string): MlxSession {
  const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1 << 30)'], { stdio: ['pipe', 'pipe', 'pipe'] });
  return new MlxSession(child, base, SAMPLING, '/fake/model-dir');
}

const chunk = (content: string): string =>
  JSON.stringify({ choices: [{ delta: { content } }] });
const usageChunk = (prompt: number, completion: number): string =>
  JSON.stringify({ choices: [], usage: { prompt_tokens: prompt, completion_tokens: completion } });

const cleanups: Array<() => Promise<void>> = [];
afterAll(async () => {
  for (const cleanup of cleanups) await cleanup();
});

async function withFake(responses: ScriptedResponse[]): Promise<FakeServer> {
  const fake = await startFakeSse(responses);
  cleanups.push(async () => {
    await fake.session.close();
    await fake.close();
  });
  return fake;
}

describe('MlxSession over a scripted SSE endpoint', () => {
  it('completes with client-observed timing and usage-only token counts', async () => {
    const fake = await withFake([
      {
        frames: [
          { payload: chunk('Vermilion'), delayMs: 30 },
          { payload: chunk('.'), delayMs: 10 },
          { payload: usageChunk(24, 3) },
          { payload: '[DONE]' },
        ],
      },
    ]);
    const result = await fake.session.invoke({ prompt: 'color?', timeoutMs: 5000 });
    expect(result.terminal).toBe('completed');
    expect(result.reply).toBe('Vermilion.');
    expect(result.report?.promptTokens).toBe(24);
    expect(result.report?.outputTokens).toBe(3);
    // Client-observed: TTFT reflects the scripted 30 ms first-frame
    // delay; generation spans first token → terminal.
    expect(result.report?.ttftMs).toBeGreaterThan(20);
    expect(result.report?.endToEndMs).toBeGreaterThan(result.report?.ttftMs ?? Infinity);
    expect(result.report?.generationDurationMs).toBeGreaterThan(0);
    // The adapter asks for usage and sends the sampling block.
    const request = fake.requests[0];
    expect(request?.['stream_options']).toEqual({ include_usage: true });
    // The model id is the exact served path — anything else makes
    // mlx_lm.server try to RESOLVE the id (a network hazard).
    expect(request?.['model']).toBe('/fake/model-dir');
    expect(request?.['temperature']).toBe(0);
    expect(request?.['max_tokens']).toBe(64);
    expect(request?.['seed']).toBe(42);
  });

  it('reports missing usage as a report without token counts', async () => {
    const fake = await withFake([
      { frames: [{ payload: chunk('hi') }, { payload: '[DONE]' }] },
    ]);
    const result = await fake.session.invoke({ prompt: 'p', timeoutMs: 5000 });
    expect(result.terminal).toBe('completed');
    expect(result.report?.promptTokens).toBeUndefined();
    expect(result.report?.outputTokens).toBeUndefined();
  });

  it('classifies a non-JSON data frame as malformed', async () => {
    const fake = await withFake([
      { frames: [{ payload: chunk('a') }, { payload: '{broken' }] },
    ]);
    const result = await fake.session.invoke({ prompt: 'p', timeoutMs: 5000 });
    expect(result.terminal).toBe('malformed');
  });

  it('classifies a destroyed stream as crashed', async () => {
    const fake = await withFake([
      { frames: [{ payload: chunk('a') }], destroy: true },
    ]);
    const result = await fake.session.invoke({ prompt: 'p', timeoutMs: 5000 });
    expect(result.terminal).toBe('crashed');
  });

  it('classifies a stream that never terminates as timedOut', async () => {
    const fake = await withFake([
      { frames: [{ payload: chunk('a') }], end: false },
    ]);
    const result = await fake.session.invoke({ prompt: 'p', timeoutMs: 400 });
    expect(result.terminal).toBe('timedOut');
  });

  it('ends timing at [DONE], not at connection closure', async () => {
    // The server sends [DONE] promptly but never closes the stream.
    // Client-observed timing must end when [DONE] is parsed — the
    // lingering connection (here: until the 5 s timeout would fire)
    // must not inflate endToEndMs or generationDurationMs.
    const fake = await withFake([
      {
        frames: [
          { payload: chunk('hi') },
          { payload: usageChunk(2, 1) },
          { payload: '[DONE]' },
        ],
        end: false,
      },
    ]);
    const result = await fake.session.invoke({ prompt: 'p', timeoutMs: 5000 });
    expect(result.terminal).toBe('completed');
    expect(result.report?.promptTokens).toBe(2);
    expect(result.report?.endToEndMs).toBeLessThan(1000);
    expect(result.report?.generationDurationMs).toBeLessThan(1000);
  });

  it('measures cancel latency from abort to conclusive close', async () => {
    const fake = await withFake([
      {
        frames: [
          { payload: chunk('one ') },
          { payload: chunk('two '), delayMs: 10 },
          { payload: chunk('never '), delayMs: 5000 },
        ],
        end: false,
      },
    ]);
    const result = await fake.session.invoke({ prompt: 'p', timeoutMs: 10_000, cancelAfterTokens: 2 });
    expect(result.terminal).toBe('cancelled');
    expect(result.cancellationLatencyMs).toBeGreaterThan(0);
    expect(result.cancellationLatencyMs).toBeLessThan(3000);
  });
});

describe('managed server lifecycle', () => {
  it('close() terminates the whole process group, not only the direct child', async () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-mlx-group-'));
    const pidFile = path.join(dir, 'grandchild.pid');
    const script = path.join(dir, 'server-stub.sh');
    // A stand-in "server" that forks a long-lived grandchild (like an
    // engine worker) before serving /health: last CLI arg is the port
    // the session appended.
    writeFileSync(
      script,
      `#!/bin/sh
for arg in "$@"; do port="$arg"; done
sleep 300 &
echo $! > "${pidFile}"
exec "${process.execPath}" -e "require('http').createServer((q, s) => s.end('ok')).listen(process.argv[1], '127.0.0.1')" "$port"
`,
    );
    chmodSync(script, 0o755);
    try {
      const session = await startMlxSession(
        { command: [script, '--model', '/x'], modelDir: '/x', startupTimeoutMs: 10_000 },
        SAMPLING,
      );
      const grandchild = Number(readFileSync(pidFile, 'utf8').trim());
      expect(Number.isInteger(grandchild) && grandchild > 0).toBe(true);
      expect(() => process.kill(grandchild, 0)).not.toThrow();
      await session.close();
      await new Promise((resolve) => setTimeout(resolve, 200));
      // Signaling only the direct child would leave this alive.
      expect(() => process.kill(grandchild, 0)).toThrow();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('sweeps the group when the leader exits during startup', async () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-mlx-earlyexit-'));
    const pidFile = path.join(dir, 'grandchild.pid');
    const script = path.join(dir, 'server-stub.sh');
    // Forks a worker, then the leader dies before ever serving
    // /health — the startup failure must not orphan the worker.
    writeFileSync(
      script,
      `#!/bin/sh
sleep 300 &
echo $! > "${pidFile}"
exit 7
`,
    );
    chmodSync(script, 0o755);
    try {
      await expect(
        startMlxSession({ command: [script, '--model', '/x'], modelDir: '/x', startupTimeoutMs: 10_000 }, SAMPLING),
      ).rejects.toThrow(/exited during startup/);
      const grandchild = Number(readFileSync(pidFile, 'utf8').trim());
      expect(Number.isInteger(grandchild) && grandchild > 0).toBe(true);
      await new Promise((resolve) => setTimeout(resolve, 200));
      expect(() => process.kill(grandchild, 0)).toThrow();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('close() sweeps the group even when the leader already died', async () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-mlx-deadleader-'));
    const pidFile = path.join(dir, 'grandchild.pid');
    const script = path.join(dir, 'server-stub.sh');
    // Serves /health, then the leader kills itself mid-session —
    // like a server crash. The worker (SIGINT-immune background job)
    // survives the leader; close() must still reap it.
    writeFileSync(
      script,
      `#!/bin/sh
for arg in "$@"; do port="$arg"; done
sleep 300 &
echo $! > "${pidFile}"
exec "${process.execPath}" -e "require('http').createServer((q, s) => s.end('ok')).listen(process.argv[1], '127.0.0.1'); setTimeout(() => process.exit(1), 1500)" "$port"
`,
    );
    chmodSync(script, 0o755);
    try {
      const session = await startMlxSession(
        { command: [script, '--model', '/x'], modelDir: '/x', startupTimeoutMs: 10_000 },
        SAMPLING,
      );
      const grandchild = Number(readFileSync(pidFile, 'utf8').trim());
      for (let i = 0; i < 100 && session.processAlive; i += 1) {
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      expect(session.processAlive).toBe(false);
      expect(() => process.kill(grandchild, 0)).not.toThrow();
      await session.close();
      await new Promise((resolve) => setTimeout(resolve, 200));
      expect(() => process.kill(grandchild, 0)).toThrow();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 20_000);
});

describe('model identity', () => {
  it('digests a model directory deterministically and detects tampering', () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-model-id-'));
    try {
      writeFileSync(path.join(dir, 'config.json'), '{"model_type":"synthetic"}');
      writeFileSync(path.join(dir, 'weights.bin'), 'not real weights');
      const first = digestModelDir(dir);
      const again = digestModelDir(dir);
      expect(first).toBe(again);
      expect(first).toMatch(/^sha256:[0-9a-f]{64}$/);
      writeFileSync(path.join(dir, 'weights.bin'), 'tampered weights!');
      expect(digestModelDir(dir)).not.toBe(first);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('refuses a model directory containing a symlink', () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-model-link-'));
    try {
      writeFileSync(path.join(dir, 'config.json'), '{}');
      symlinkSync('/etc/hosts', path.join(dir, 'sneaky'));
      expect(() => digestModelDir(dir)).toThrow(/symlink/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
