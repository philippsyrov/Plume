// D132: benchmark results viewer. Read-only display of the open
// project's benchmark evidence: JSONL attempt records (D129) and the
// model catalog / presets (D131). Analysis comes from the SAME
// summarizer library the CLI uses — groups, pairs, and refusals here
// are byte-for-byte the CLI's verdicts, never a friendlier retelling.
//
// Honesty posture:
//   * Fake-runtime records are bannered as harness test data, exactly
//     like the CLI summarizer.
//   * Refused/incomplete groups show the refusal, not partial stats.
//   * null probe values render as "—", never as zero.
//   * Running benchmarks stays a terminal action
//     (scripts/benchmark-preset.sh); this panel never launches runs.

import { useEffect, useState, type ReactNode } from 'react';

import { readFile } from '../../lib/api/fs';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { formatBytes } from '../../lib/format';
import type { Stats } from '../../../scripts/benchmark/summarize-lib.ts';
import type { BenchmarkRecord } from '../../../scripts/benchmark/types.ts';
import type { Catalog } from '../../../scripts/benchmark/catalog-schema.ts';
import type { BenchmarkEvidence, CatalogState, ResultFile } from './data';
import { useBenchmarkEvidence } from './useBenchmarkEvidence';

export function BenchmarksPanel() {
  const { state, refresh } = useBenchmarkEvidence();
  const [preview, setPreview] = useState<string | null>(null);

  return (
    <section className="plume-benchmarks" aria-label="Benchmark results">
      <header className="plume-benchmarks-header">
        <div>
          <h2>Benchmarks</h2>
          <p>
            Local evidence read from this project&apos;s{' '}
            <code>benchmark-artifacts/</code> — reader-validated, read-only.
            Runs happen in a terminal via <code>scripts/benchmark-preset.sh</code>.
          </p>
        </div>
        <button
          type="button"
          className="ink-button"
          onClick={refresh}
          disabled={state.kind === 'loading'}
        >
          {state.kind === 'loading' ? 'Loading…' : 'Refresh'}
        </button>
      </header>

      {state.kind === 'loading' ? (
        <p role="status">Loading benchmark evidence…</p>
      ) : state.kind === 'error' ? (
        <p className="plume-benchmarks-error" role="alert">
          {state.message}
        </p>
      ) : (
        <EvidenceView evidence={state.evidence} onPreview={setPreview} />
      )}

      {preview !== null ? (
        <EvidencePreview path={preview} onClose={() => setPreview(null)} />
      ) : null}
    </section>
  );
}

function EvidenceView({
  evidence,
  onPreview,
}: {
  evidence: BenchmarkEvidence;
  onPreview: (path: string) => void;
}) {
  const noArtifacts = evidence.artifacts.kind === 'absent';
  const noCatalog = evidence.catalog.kind === 'absent';
  if (noArtifacts && noCatalog) {
    return (
      <p className="plume-benchmarks-empty" role="status">
        This project has no benchmark evidence yet — no{' '}
        <code>benchmark-artifacts/</code> directory and no{' '}
        <code>benchmarks/catalog/</code>. Run a preset from a terminal to
        produce some: <code>scripts/benchmark-preset.sh pong-paired-smoke</code>.
      </p>
    );
  }
  return (
    <>
      <CatalogSection catalog={evidence.catalog} />
      {evidence.artifacts.kind === 'absent' ? (
        <p className="plume-benchmarks-empty" role="status">
          No <code>benchmark-artifacts/</code> directory — no runs recorded on
          this machine yet.
        </p>
      ) : evidence.artifacts.kind === 'refused' ? (
        <p className="plume-benchmarks-error" role="alert">
          Results refused: {evidence.artifacts.message}
        </p>
      ) : evidence.artifacts.files.length === 0 ? (
        <p className="plume-benchmarks-empty" role="status">
          <code>benchmark-artifacts/</code> exists but holds no{' '}
          <code>.jsonl</code> record files.
        </p>
      ) : (
        evidence.artifacts.files.map((file) => (
          <ResultFileSection key={file.path} file={file} onPreview={onPreview} />
        ))
      )}
    </>
  );
}

/// Every table renders inside this container, which owns horizontal
/// overflow: the card never grows past the workspace width and every
/// column stays reachable at constrained window sizes (Codex's
/// packaged constrained-window finding — the nine-column attempts
/// table was clipped with no scroll owner). Focusable so the scroll
/// is keyboard-reachable.
function TableScroll({ children }: { children: ReactNode }) {
  return (
    <div className="plume-benchmarks-table-scroll" tabIndex={0}>
      {children}
    </div>
  );
}

// ---- Catalog -------------------------------------------------------------

function CatalogSection({ catalog }: { catalog: CatalogState }) {
  if (catalog.kind === 'absent') {
    return (
      <p className="plume-benchmarks-empty" role="status">
        No model catalog (<code>benchmarks/catalog/</code>) in this project.
      </p>
    );
  }
  if (catalog.kind === 'error') {
    return (
      <div className="plume-benchmarks-section">
        <h3>Model catalog</h3>
        <p className="plume-benchmarks-error" role="alert">
          Catalog refused: {catalog.message}
        </p>
      </div>
    );
  }
  return <CatalogTables catalog={catalog.catalog} />;
}

function CatalogTables({ catalog }: { catalog: Catalog }) {
  return (
    <div className="plume-benchmarks-section">
      <h3>Model catalog</h3>
      <TableScroll>
      <table className="plume-benchmarks-table">
        <caption>Cataloged models with pinned artifact identity</caption>
        <thead>
          <tr>
            <th scope="col">Model</th>
            <th scope="col">Engine</th>
            <th scope="col">Quantization</th>
            <th scope="col">Max context</th>
            <th scope="col">Pinned digest</th>
          </tr>
        </thead>
        <tbody>
          {[...catalog.models.values()].map((model) => (
            <tr key={model.id}>
              <td>
                <strong>{model.displayName}</strong>{' '}
                <code>{model.id}</code>
              </td>
              <td>{model.engine}</td>
              <td>{formatQuantization(model.artifact)}</td>
              <td>{model.maxContextTokens.toLocaleString()}</td>
              <td>
                <code title={model.artifact.sha256}>
                  {shortDigest(model.artifact.sha256)}
                </code>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      </TableScroll>
      <TableScroll>
      <table className="plume-benchmarks-table">
        <caption>Runnable presets (select-and-run from a terminal)</caption>
        <thead>
          <tr>
            <th scope="col">Preset</th>
            <th scope="col">Model</th>
            <th scope="col">Paths</th>
            <th scope="col">Suites</th>
            <th scope="col">Description</th>
          </tr>
        </thead>
        <tbody>
          {[...catalog.presets.values()].map((preset) => (
            <tr key={preset.id}>
              <td>
                <code>{preset.id}</code>
              </td>
              <td>
                <code>{preset.model}</code>
              </td>
              <td>{preset.measurementPaths.join(', ')}</td>
              <td>{preset.suites.length}</td>
              <td className="plume-benchmarks-description">{preset.description}</td>
            </tr>
          ))}
        </tbody>
      </table>
      </TableScroll>
    </div>
  );
}

// ---- Result files ----------------------------------------------------------

function ResultFileSection({
  file,
  onPreview,
}: {
  file: ResultFile;
  onPreview: (path: string) => void;
}) {
  const failures = collectFailures(file);
  return (
    <article className="plume-benchmarks-section" aria-label={`Run ${file.runLabel}`}>
      <h3>
        {file.runLabel} <code className="plume-benchmarks-path">{file.path}</code>
      </h3>
      {file.hasFakeRuntime ? (
        <p className="plume-benchmarks-fake-banner" role="note">
          HARNESS TEST DATA — records from the scripted fake runtime. These are
          harness-mechanics results, not model measurements.
        </p>
      ) : null}
      {file.readError !== null ? (
        <p className="plume-benchmarks-error" role="alert">
          File refused: {file.readError}
        </p>
      ) : (
        <>
          <GroupsTable file={file} />
          <PairsTable file={file} />
          {failures.length > 0 ? (
            <div className="plume-benchmarks-failures">
              <h4>Failures &amp; refusals</h4>
              <ul>
                {failures.map((failure) => (
                  <li key={failure} className="plume-benchmarks-error">
                    {failure}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          <AttemptsList file={file} onPreview={onPreview} />
        </>
      )}
    </article>
  );
}

/// Everything about this file the user should see as "not clean":
/// invalid lines, refused/inconsistent groups, and failed attempts.
function collectFailures(file: ResultFile): string[] {
  const failures: string[] = [];
  failures.push(...file.lineErrors.map((e) => `invalid record — ${e}`));
  for (const group of file.groups) {
    failures.push(...group.configErrors);
    if (group.incomplete) {
      failures.push(
        `group ${group.groupId}/${group.population}: incomplete evidence ` +
          `(${group.completed} completed of at least 3) — no statistics`,
      );
    }
  }
  for (const record of file.records) {
    if (record.outcome.status === 'passed') continue;
    const errorClass =
      record.outcome.errorClass !== null ? ` (${record.outcome.errorClass})` : '';
    failures.push(
      `attempt ${record.run.id} [${record.suite.id} ${record.run.population} ` +
        `rep ${record.run.repetition}]: ${record.outcome.status}${errorClass}`,
    );
  }
  return failures;
}

function GroupsTable({ file }: { file: ResultFile }) {
  if (file.groups.length === 0) {
    return (
      <p className="plume-benchmarks-empty" role="status">
        No valid records in this file.
      </p>
    );
  }
  return (
    <TableScroll>
      <table className="plume-benchmarks-table">
      <caption>Comparison groups (medians over completed, included attempts)</caption>
      <thead>
        <tr>
          <th scope="col">Group</th>
          <th scope="col">Population</th>
          <th scope="col">Suite</th>
          <th scope="col">Engine</th>
          <th scope="col">Completed</th>
          <th scope="col">Included</th>
          <th scope="col">endToEndMs</th>
          <th scope="col">gen tok/s</th>
        </tr>
      </thead>
      <tbody>
        {file.groups.map((group) => {
          const stats = (value: Stats | null): string =>
            group.refused ? 'refused (inconsistent group)' : formatStats(value);
          return (
            <tr key={`${group.groupId}-${group.population}`}>
              <td>
                <code>{group.groupId}</code>
              </td>
              <td>{group.population}</td>
              <td>{group.suiteId}</td>
              <td>{group.engine}</td>
              <td>
                {group.completed}
                {group.incomplete ? ' (incomplete evidence)' : ''}
              </td>
              <td>{group.included}</td>
              <td>{stats(group.endToEndMs)}</td>
              <td>{stats(group.generationTokensPerSecond)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
      </TableScroll>
  );
}

function PairsTable({ file }: { file: ResultFile }) {
  if (file.pairs.length === 0) return null;
  return (
    <TableScroll>
      <table className="plume-benchmarks-table">
      <caption>Raw vs Plume pairs (extraOverheadMs = plume − raw, valid pairs only)</caption>
      <thead>
        <tr>
          <th scope="col">Pair</th>
          <th scope="col">Valid</th>
          <th scope="col">extraOverheadMs</th>
          <th scope="col">Note</th>
        </tr>
      </thead>
      <tbody>
        {file.pairs.map((pair) => (
          <tr key={pair.pairId}>
            <td>
              <code>{pair.pairId}</code>
            </td>
            <td>{pair.valid ? 'yes' : 'no'}</td>
            <td>{pair.extraOverheadMs === null ? '—' : pair.extraOverheadMs.toFixed(1)}</td>
            <td>{pair.reason ?? ''}</td>
          </tr>
        ))}
      </tbody>
    </table>
      </TableScroll>
  );
}

function AttemptsList({
  file,
  onPreview,
}: {
  file: ResultFile;
  onPreview: (path: string) => void;
}) {
  if (file.records.length === 0) return null;
  return (
    <details className="plume-benchmarks-attempts">
      <summary>
        Attempts ({file.records.length}) — per-attempt timing, resources, evidence
      </summary>
      <TableScroll>
      <table className="plume-benchmarks-table">
        <caption>Individual attempts; — means the probe reported no value</caption>
        <thead>
          <tr>
            <th scope="col">Attempt</th>
            <th scope="col">Group</th>
            <th scope="col">Rep</th>
            <th scope="col">Status</th>
            <th scope="col">endToEndMs</th>
            <th scope="col">Peak memory</th>
            <th scope="col">Swap Δ</th>
            <th scope="col">Thermal</th>
            <th scope="col">Evidence</th>
          </tr>
        </thead>
        <tbody>
          {file.records.map((record) => (
            <AttemptRow key={record.run.id} record={record} onPreview={onPreview} />
          ))}
        </tbody>
      </table>
      </TableScroll>
    </details>
  );
}

function AttemptRow({
  record,
  onPreview,
}: {
  record: BenchmarkRecord;
  onPreview: (path: string) => void;
}) {
  const excluded = !record.includeInSummary;
  return (
    <tr className={excluded ? 'plume-benchmarks-excluded' : undefined}>
      <td>
        <code>{record.run.id}</code>
        {excluded ? (
          <span className="plume-benchmarks-exclusion">
            {' '}
            excluded{record.exclusionReason !== null ? `: ${record.exclusionReason}` : ''}
          </span>
        ) : null}
      </td>
      <td>
        <code>{record.run.groupId}</code> {record.run.population}
      </td>
      <td>{record.run.repetition}</td>
      <td>
        <span className={`ink-badge plume-benchmarks-status-${record.outcome.status}`}>
          {record.outcome.status}
        </span>
      </td>
      <td>{formatNumber(record.timing.endToEndMs)}</td>
      <td>{formatNullableBytes(record.resources.peakUnifiedMemoryBytes)}</td>
      <td>{formatSwapDelta(record.resources.swapDeltaBytes)}</td>
      <td>{formatThermal(record.host.thermalStart, record.resources.thermalEnd)}</td>
      <td>
        {record.artifacts.length === 0
          ? '—'
          : record.artifacts.map((ref) => (
              <button
                key={ref}
                type="button"
                className="plume-benchmarks-evidence-link"
                onClick={() => onPreview(ref)}
                title={ref}
              >
                {lastPathSegment(ref)}
              </button>
            ))}
      </td>
    </tr>
  );
}

// ---- Evidence preview -------------------------------------------------------

type PreviewState =
  | { kind: 'loading' }
  | { kind: 'ready'; text: string }
  | { kind: 'error'; message: string };

function EvidencePreview({ path, onClose }: { path: string; onClose: () => void }) {
  const [state, setState] = useState<PreviewState>({ kind: 'loading' });
  useEffect(() => {
    let cancelled = false;
    readFile(path)
      .then((content) => {
        if (cancelled) return;
        if (content.encoding !== 'utf-8') {
          setState({ kind: 'error', message: 'Not a UTF-8 text file.' });
          return;
        }
        setState({ kind: 'ready', text: content.content });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setState({ kind: 'error', message: previewError(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [path]);
  return (
    <div
      className="plume-benchmarks-preview-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="plume-benchmarks-preview"
        role="dialog"
        aria-modal="true"
        aria-label={`Evidence file ${path}`}
      >
        <header className="plume-benchmarks-preview-header">
          <code>{path}</code>
          <button type="button" className="ink-button" onClick={onClose}>
            Close
          </button>
        </header>
        {state.kind === 'loading' ? (
          <p role="status">Loading…</p>
        ) : state.kind === 'error' ? (
          <p className="plume-benchmarks-error" role="alert">
            {state.message}
          </p>
        ) : (
          <pre className="plume-benchmarks-preview-body">{state.text}</pre>
        )}
      </div>
    </div>
  );
}

function previewError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to read evidence file.';
}

// ---- Formatting -------------------------------------------------------------

/// Mirrors the CLI summarizer's stats cell: median with min/max/IQR/n.
function formatStats(stats: Stats | null): string {
  if (stats === null) return '—';
  const iqr = stats.iqr === null ? 'n<4' : stats.iqr.toFixed(1);
  return `${stats.median.toFixed(1)} (min ${stats.min.toFixed(1)}, max ${stats.max.toFixed(1)}, IQR ${iqr}, n=${stats.count})`;
}

function formatNumber(value: number | null): string {
  return value === null ? '—' : value.toFixed(1);
}

function formatNullableBytes(bytes: number | null): string {
  return bytes === null ? '—' : formatBytes(bytes);
}

/// Swap delta is signed: negative means swap shrank during the run.
function formatSwapDelta(bytes: number | null): string {
  if (bytes === null) return '—';
  if (bytes === 0) return '0 B';
  const sign = bytes > 0 ? '+' : '−';
  return `${sign}${formatBytes(Math.abs(bytes))}`;
}

function formatThermal(start: string | null, end: string | null): string {
  if (start === null && end === null) return '—';
  return `${start ?? '—'} → ${end ?? '—'}`;
}

function formatQuantization(artifact: {
  quantizationMethod: string | null;
  quantizationBits: number | null;
  quantizationGroupSize: number | null;
}): string {
  if (artifact.quantizationMethod === null && artifact.quantizationBits === null) {
    return '—';
  }
  const parts: string[] = [];
  if (artifact.quantizationMethod !== null) parts.push(artifact.quantizationMethod);
  if (artifact.quantizationBits !== null) parts.push(`${artifact.quantizationBits}-bit`);
  if (artifact.quantizationGroupSize !== null) {
    parts.push(`group ${artifact.quantizationGroupSize}`);
  }
  return parts.join(', ');
}

function shortDigest(sha256: string): string {
  return sha256.replace('sha256:', '').slice(0, 12);
}

function lastPathSegment(ref: string): string {
  const parts = ref.split('/');
  return parts[parts.length - 1] ?? ref;
}
