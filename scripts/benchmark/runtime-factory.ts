// D129A: transport-based runtime resolution. One place decides, per
// sanitized config, HOW sessions are created, WHICH timing method the
// records carry, and — for real runtimes — verifies the declared
// identity against reality before anything runs:
//
//   * `stdio-jsonl` — the scripted fake runtime. Declared identity is
//     recorded as-is (its artifact digest is the case script's real
//     sha256, pinned by the test-support builder). Timing is
//     runtime-reported.
//   * `openai-sse` — a Plume-managed `mlx_lm.server`. The model
//     directory is re-digested and must equal the declared
//     `model.artifact.sha256`; the interpreter's actual
//     `mlx_lm.__version__` is probed and must match a declared
//     version (or fills a null one). A mismatch refuses the run —
//     records never carry an unverified identity. Timing is
//     client-observed.

import { digestModelDir, probeMlxLmVersion } from './model-identity.ts';
import { startMlxSession } from './mlx-runtime.ts';
import type { MlxServerConfig } from './mlx-runtime.ts';
import { probeHealth, runInvocation, RuntimeSession } from './runtime-client.ts';
import type { InvocationResult, InvokeOptions } from './runtime-client.ts';
import type { ModelBlock, RuntimeBlock, RuntimeConfigurationBlock } from './types.ts';

export interface HarnessRuntimeConfig {
  path: string;
  name: string;
  version: string | null;
  engine: string;
  backend: string;
  transport: string;
  /// stdio-jsonl: the subprocess command.
  command?: string[];
  /// openai-sse: the managed server.
  server?: MlxServerConfig;
  configuration: RuntimeConfigurationBlock;
}

export interface HarnessConfig {
  measurementPath: 'rawRuntime';
  runtime: HarnessRuntimeConfig;
  model: ModelBlock;
}

/// What every transport's session must provide.
export interface BenchmarkRuntime {
  invoke(options: InvokeOptions): Promise<InvocationResult>;
  close(): void | Promise<void>;
}

export interface CrashRecovery {
  healthy: boolean;
  followUpPassed: boolean;
}

export interface ResolvedRuntime {
  /// The verified identity block that goes into every record.
  block: RuntimeBlock;
  timingMethod: 'runtimeReported' | 'clientObserved';
  createSession(): Promise<BenchmarkRuntime>;
  /// Post-crash restart probe + follow-up request (the
  /// cancellation-restart suite's recovery evidence).
  crashRestart(timeoutMs: number): Promise<CrashRecovery>;
}

const versionCache = new Map<string, string | null>();

function probedVersion(pythonBin: string): string | null {
  const cached = versionCache.get(pythonBin);
  if (cached !== undefined) return cached;
  const version = probeMlxLmVersion(pythonBin);
  versionCache.set(pythonBin, version);
  return version;
}

export async function resolveRuntime(config: HarnessConfig): Promise<ResolvedRuntime> {
  const runtime = config.runtime;

  if (runtime.transport === 'stdio-jsonl') {
    const command = runtime.command;
    if (!Array.isArray(command) || command.length === 0) {
      throw new Error('stdio-jsonl runtime needs a non-empty command array');
    }
    return {
      block: recordBlock(runtime, runtime.version),
      timingMethod: 'runtimeReported',
      createSession: () => Promise.resolve(new RuntimeSession(command)),
      crashRestart: async (timeoutMs) => {
        const healthy = await probeHealth(command, timeoutMs);
        if (!healthy) return { healthy, followUpPassed: false };
        const followUp = await runInvocation(command, { prompt: 'follow-up', timeoutMs }, true);
        return { healthy, followUpPassed: followUp.terminal === 'completed' && followUp.reply.length > 0 };
      },
    };
  }

  if (runtime.transport === 'openai-sse') {
    const server = runtime.server;
    if (server === undefined || !Array.isArray(server.command) || server.command.length === 0) {
      throw new Error('openai-sse runtime needs a server block with a non-empty command array');
    }
    // The digested directory and the directory the server actually
    // loads must be the same path, or identity verification would
    // vouch for bytes the server never serves. Strict form: exactly
    // ONE `--model` token followed by server.modelDir, and no
    // `--model=` variant — argparse lets a later duplicate silently
    // win, which would load bytes the digest never described.
    const modelFlagPositions = server.command
      .map((arg, index) => (arg === '--model' || arg.startsWith('--model=') ? index : -1))
      .filter((index) => index !== -1);
    const flagIndex = modelFlagPositions[0];
    if (
      modelFlagPositions.length !== 1 ||
      flagIndex === undefined ||
      server.command[flagIndex] !== '--model' ||
      server.command[flagIndex + 1] !== server.modelDir
    ) {
      throw new Error(
        'server.command must pass a single --model with exactly server.modelDir ' +
          '(two-token form, no duplicates, no --model=) — a duplicate would make ' +
          'the server load bytes the digest never described',
      );
    }
    // Verified artifact identity: the directory the server will load,
    // re-digested IN FULL (deliberately no cache — see
    // model-identity.ts), must be exactly what the config declares.
    // Runs at resolve time AND before every server launch — a
    // checkpoint changed between declaration and launch refuses
    // instead of running under the stale digest.
    const verifyArtifact = (): void => {
      const actualDigest = digestModelDir(server.modelDir);
      if (actualDigest !== config.model.artifact.sha256) {
        throw new Error(
          `model identity mismatch: config declares ${config.model.artifact.sha256} but ` +
            `${server.modelDir} hashes to ${actualDigest} — refusing to record an unverified identity`,
        );
      }
    };
    verifyArtifact();
    // Verified engine identity: probe the interpreter that will
    // serve. This adapter can only verify mlx-lm — any other engine
    // declaration over openai-sse would be recorded unprobed, so it
    // is refused outright.
    if (runtime.engine !== 'mlx-lm') {
      throw new Error(
        `openai-sse transport verifies engine identity for "mlx-lm" only — ` +
          `got ${JSON.stringify(runtime.engine)}; refusing to record an unverified engine identity`,
      );
    }
    const interpreter = server.command[0];
    if (interpreter === undefined) throw new Error('server command must not be empty');
    const probed = probedVersion(interpreter);
    if (probed === null) {
      throw new Error(`cannot import mlx_lm with ${interpreter} — refusing to run without a verified engine`);
    }
    if (runtime.version !== null && runtime.version !== probed) {
      throw new Error(
        `engine version mismatch: config declares mlx-lm ${runtime.version} but ${interpreter} serves ${probed}`,
      );
    }
    return {
      block: recordBlock(runtime, probed),
      timingMethod: 'clientObserved',
      createSession: async () => {
        verifyArtifact();
        return startMlxSession(server, config.model.sampling);
      },
      crashRestart: async (timeoutMs) => {
        // Restart = a fresh managed server reaching health, then one
        // completed follow-up generation on it. Identity is
        // re-verified like any other launch.
        verifyArtifact();
        let session;
        try {
          session = await startMlxSession(server, config.model.sampling);
        } catch {
          return { healthy: false, followUpPassed: false };
        }
        try {
          const followUp = await session.invoke({ prompt: 'Reply with the single word: pong.', timeoutMs });
          return { healthy: true, followUpPassed: followUp.terminal === 'completed' && followUp.reply.length > 0 };
        } finally {
          await session.close();
        }
      },
    };
  }

  throw new Error(`unknown runtime transport ${JSON.stringify(runtime.transport)}`);
}

function recordBlock(runtime: HarnessRuntimeConfig, version: string | null): RuntimeBlock {
  return {
    path: runtime.path,
    name: runtime.name,
    version,
    engine: runtime.engine,
    backend: runtime.backend,
    configuration: runtime.configuration,
    transport: runtime.transport,
  };
}
