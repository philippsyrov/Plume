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
//
// D129C: the same `openai-sse` runtime serves TWO measurement paths.
// `rawRuntime` talks to the server with the harness's direct client
// (MlxSession). `plumeOrchestration` puts Plume's own code between:
// the verified server plus a `plume_bench orchestrate` sidecar built
// from Plume's real modules (assemble → Plume's TCP/SSE client →
// UI-facing emission). Identity verification is identical for both;
// the plume path ADDITIONALLY verifies — via the sidecar's health
// handshake — that the declared sampling matches what Plume really
// sends (no client sampling controls, Plume's own max_tokens cap).

import { digestModelDir, plumeIdentity, probeMlxLmVersion, verifySidecarIdentity } from './model-identity.ts';
import type { PlumeIdentity } from './model-identity.ts';
import { startMlxSession } from './mlx-runtime.ts';
import type { MlxServerConfig, MlxSession } from './mlx-runtime.ts';
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
  measurementPath: 'rawRuntime' | 'plumeOrchestration';
  runtime: HarnessRuntimeConfig;
  model: ModelBlock;
  /// plumeOrchestration only: the built plume_bench sidecar.
  plumeBench?: { binary: string };
}

/// What every transport's session must provide. `launchIdentity` is
/// set only by sessions whose measurements flow through a verified
/// plume_bench sidecar: it is the Plume identity the sidecar was
/// verified against AT LAUNCH, and every attempt served by the
/// session must pin exactly that identity on its record (runOne
/// refuses drift — e.g. a commit landing mid-suite).
export interface BenchmarkRuntime {
  invoke(options: InvokeOptions): Promise<InvocationResult>;
  close(): void | Promise<void>;
  launchIdentity?: PlumeIdentity;
}

export interface CrashRecovery {
  healthy: boolean;
  followUpPassed: boolean;
}

export interface ResolvedRuntime {
  /// The verified identity block that goes into every record.
  block: RuntimeBlock;
  timingMethod: 'runtimeReported' | 'clientObserved';
  /// Whether runOne should sample machine resource probes around the
  /// measured request (D129B). True only for real runtimes — probing
  /// around the scripted fake would make its deterministic fixture
  /// records depend on the machine running the tests.
  supportsResourceProbes: boolean;
  createSession(): Promise<BenchmarkRuntime>;
  /// Post-crash restart probe + follow-up request (the
  /// cancellation-restart suite's recovery evidence).
  crashRestart(timeoutMs: number): Promise<CrashRecovery>;
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
      supportsResourceProbes: false,
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
    const probed = probeMlxLmVersion(interpreter);
    if (probed === null) {
      throw new Error(`cannot import mlx_lm with ${interpreter} — refusing to run without a verified engine`);
    }
    if (runtime.version !== null && runtime.version !== probed) {
      throw new Error(
        `engine version mismatch: config declares mlx-lm ${runtime.version} but ${interpreter} serves ${probed}`,
      );
    }
    // Like the artifact digest, the engine probe is NEVER cached: the
    // interpreter environment can be upgraded mid-suite, and a later
    // server would then run new code while records carry the version
    // resolved earlier. Every launch re-probes and must still see
    // exactly the resolved version.
    const verifyEngine = (): void => {
      const now = probeMlxLmVersion(interpreter);
      if (now !== probed) {
        throw new Error(
          `engine identity changed since resolve: ${interpreter} now serves ` +
            `mlx-lm ${now ?? '(import failed)'} but records would carry ${probed} — refusing to launch`,
        );
      }
    };
    if (config.measurementPath === 'rawRuntime') {
      return {
        block: recordBlock(runtime, probed),
        timingMethod: 'clientObserved',
        supportsResourceProbes: true,
        createSession: async () => {
          verifyArtifact();
          verifyEngine();
          return startMlxSession(server, config.model.sampling);
        },
        crashRestart: async (timeoutMs) => {
          // Restart = a fresh managed server reaching health, then one
          // completed follow-up generation on it. Identity is
          // re-verified like any other launch.
          verifyArtifact();
          verifyEngine();
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

    // D129C: plumeOrchestration — the verified server plus Plume's own
    // code path in between (plume_bench orchestrate). One session =
    // one server + one sidecar process, so warm/cold semantics carry
    // over: warm reuses both, cold restarts both.
    const bench = config.plumeBench;
    if (bench === undefined || typeof bench.binary !== 'string' || bench.binary.length === 0) {
      throw new Error(
        'plumeOrchestration needs plumeBench.binary (build it: ' +
          './scripts/dev-env.sh cargo build --manifest-path src-tauri/Cargo.toml --bin plume_bench)',
      );
    }
    // Every verification pins a FRESH identity snapshot taken at that
    // moment — resolve-time here, launch-time below — so nothing is
    // compared against an identity captured earlier than its use.
    await verifyPlumePosture(bench.binary, config.model, plumeIdentity());
    const createPlumeSession = async (): Promise<BenchmarkRuntime> => {
      verifyArtifact();
      verifyEngine();
      // Launch snapshot: verified immediately before the sidecar is
      // spawned and carried by the session, so every attempt it later
      // serves can require ITS identity to be exactly this one.
      const launchIdentity = plumeIdentity();
      await verifyPlumePosture(bench.binary, config.model, launchIdentity);
      const mlxServer = await startMlxSession(server, config.model.sampling);
      try {
        const sidecar = new RuntimeSession([
          bench.binary,
          'orchestrate',
          '--port',
          String(mlxServer.port),
          '--model',
          server.modelDir,
        ]);
        return new PlumeOrchestrationSession(mlxServer, sidecar, launchIdentity);
      } catch (err) {
        await mlxServer.close();
        throw err;
      }
    };
    return {
      block: recordBlock(runtime, probed),
      // Timings come from the plume_bench process itself, measured
      // monotonically at Plume's UI-facing emission boundary — the
      // measured system reporting, hence runtimeReported.
      timingMethod: 'runtimeReported',
      supportsResourceProbes: true,
      createSession: createPlumeSession,
      crashRestart: async (timeoutMs) => {
        let session: BenchmarkRuntime;
        try {
          session = await createPlumeSession();
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

/// One plumeOrchestration session: the verified mlx server plus the
/// plume_bench sidecar that talks to it through Plume's real modules.
class PlumeOrchestrationSession implements BenchmarkRuntime {
  constructor(
    private readonly server: MlxSession,
    private readonly sidecar: RuntimeSession,
    readonly launchIdentity: PlumeIdentity,
  ) {}

  invoke(options: InvokeOptions): Promise<InvocationResult> {
    return this.sidecar.invoke(options);
  }

  async close(): Promise<void> {
    this.sidecar.close();
    await this.server.close();
  }
}

/// Refuse a plumeOrchestration config whose declared generation
/// controls differ from what Plume actually puts on the wire: Plume's
/// chat path sends NO client sampling controls and its own explicit
/// max_tokens cap. One identity handshake per verification does two
/// jobs — sidecar PROVENANCE (embedded build sha + dirty must equal
/// the identity the records will carry; stale or foreign binaries
/// refuse) and the declared-equals-wired output cap.
async function verifyPlumePosture(binary: string, model: ModelBlock, expected: PlumeIdentity): Promise<void> {
  const health = verifySidecarIdentity(binary, expected);
  const sampling = model.sampling;
  const clientControls = ['temperature', 'topP', 'topK', 'minP', 'repeatPenalty', 'seed'] as const;
  for (const control of clientControls) {
    if (sampling[control] !== null) {
      throw new Error(
        `plumeOrchestration cannot honor sampling.${control} — Plume's chat path sends no client ` +
          'sampling controls; declare it null or measure rawRuntime instead',
      );
    }
  }
  if (sampling.stopSequences.length > 0) {
    throw new Error('plumeOrchestration cannot honor stopSequences — Plume sends none');
  }
  if (sampling.maxOutputTokens !== health.maxOutputTokens) {
    throw new Error(
      `plumeOrchestration output cap mismatch: config declares ${sampling.maxOutputTokens} but Plume ` +
        `actually sends max_tokens ${health.maxOutputTokens} — refusing to record a cap that is not on the wire`,
    );
  }
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
