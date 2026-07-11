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

import { digestModelDirCached, probeMlxLmVersion } from './model-identity.ts';
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
    // vouch for bytes the server never serves.
    const modelFlag = server.command.indexOf('--model');
    if (modelFlag === -1 || server.command[modelFlag + 1] !== server.modelDir) {
      throw new Error(
        'server.command must pass --model with exactly server.modelDir — ' +
          'the digest must describe the directory the server loads',
      );
    }
    // Verified artifact identity: the directory the server will load,
    // re-digested, must be exactly what the config declares.
    const actualDigest = digestModelDirCached(server.modelDir);
    if (actualDigest !== config.model.artifact.sha256) {
      throw new Error(
        `model identity mismatch: config declares ${config.model.artifact.sha256} but ` +
          `${server.modelDir} hashes to ${actualDigest} — refusing to record an unverified identity`,
      );
    }
    // Verified engine identity: probe the interpreter that will serve.
    let version = runtime.version;
    if (runtime.engine === 'mlx-lm') {
      const interpreter = server.command[0];
      if (interpreter === undefined) throw new Error('server command must not be empty');
      const probed = probedVersion(interpreter);
      if (probed === null) {
        throw new Error(`cannot import mlx_lm with ${interpreter} — refusing to run without a verified engine`);
      }
      if (version !== null && version !== probed) {
        throw new Error(
          `engine version mismatch: config declares mlx-lm ${version} but ${interpreter} serves ${probed}`,
        );
      }
      version = probed;
    }
    const resolvedVersion = version;
    return {
      block: recordBlock(runtime, resolvedVersion),
      timingMethod: 'clientObserved',
      createSession: () => startMlxSession(server, config.model.sampling),
      crashRestart: async (timeoutMs) => {
        // Restart = a fresh managed server reaching health, then one
        // completed follow-up generation on it.
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
