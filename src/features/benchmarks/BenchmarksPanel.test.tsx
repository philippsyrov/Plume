// D132: benchmark results viewer panel. Same in-memory fs tree as the
// data-layer tests; assertions here are about what the USER sees —
// banners, refusals, null probes as "—" (never zero), evidence
// preview, refresh.

import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { makeValidRecord } from '../../../scripts/benchmark/example-record.ts';
import type { BenchmarkRecord } from '../../../scripts/benchmark/types.ts';

const mocks = vi.hoisted(() => ({
  listDir: vi.fn(),
  readFile: vi.fn(),
}));

vi.mock('../../lib/api/fs', () => ({
  listDir: mocks.listDir,
  readFile: mocks.readFile,
}));

import { BenchmarksPanel } from './BenchmarksPanel';

function installTree(tree: Record<string, string>): void {
  const filePaths = Object.keys(tree);
  mocks.listDir.mockImplementation((dir: string) => {
    const prefix = `${dir}/`;
    const children = new Map<string, 'file' | 'dir'>();
    for (const filePath of filePaths) {
      if (!filePath.startsWith(prefix)) continue;
      const rest = filePath.slice(prefix.length);
      const name = rest.split('/')[0] ?? rest;
      children.set(name, rest.includes('/') ? 'dir' : 'file');
    }
    if (children.size === 0) {
      return Promise.reject({ kind: 'NotFound', details: dir });
    }
    return Promise.resolve(
      [...children.entries()].map(([name, kind]) => ({
        name,
        path: `/project/${dir}/${name}`,
        kind,
        size: kind === 'file' ? 1 : null,
        modifiedMs: 0,
      })),
    );
  });
  mocks.readFile.mockImplementation((path: string) => {
    const content = tree[path];
    if (content === undefined) {
      return Promise.reject({ kind: 'NotFound', details: path });
    }
    return Promise.resolve({ content, encoding: 'utf-8', bytes: content.length });
  });
}

function recordLine(mutate?: (record: BenchmarkRecord) => void): string {
  const record = makeValidRecord();
  if (mutate) mutate(record);
  return JSON.stringify(record);
}

beforeEach(() => {
  mocks.listDir.mockReset();
  mocks.readFile.mockReset();
});

describe('BenchmarksPanel', () => {
  it('shows the empty state when the project has no benchmark evidence', async () => {
    installTree({});
    render(<BenchmarksPanel />);
    expect(
      await screen.findByText(/no benchmark evidence yet/i),
    ).toBeInTheDocument();
  });

  it('banners fake-runtime records as harness test data', async () => {
    installTree({ 'benchmark-artifacts/run/records.jsonl': recordLine() });
    render(<BenchmarksPanel />);
    expect(await screen.findByText(/HARNESS TEST DATA/)).toBeInTheDocument();
  });

  it('renders group medians and per-attempt resources, with — for null probes', async () => {
    const lines = [1, 2, 3].map((rep) =>
      recordLine((r) => {
        r.run.id = `bench_0${rep}`;
        r.run.repetition = rep;
        if (rep === 1) {
          // Probe failure: null, never zero (D129B contract).
          r.resources.peakUnifiedMemoryBytes = null;
          r.resources.swapDeltaBytes = null;
          r.resources.thermalEnd = null;
          r.host.thermalStart = null;
        } else {
          r.resources.peakUnifiedMemoryBytes = 2 * 1024 * 1024 * 1024;
          r.resources.swapDeltaBytes = -1024;
        }
      }),
    );
    installTree({ 'benchmark-artifacts/run/records.jsonl': lines.join('\n') });
    render(<BenchmarksPanel />);
    // Group median over the three attempts (real summarizer output).
    expect(await screen.findByText(/55\.0 \(min 55\.0, max 55\.0/)).toBeInTheDocument();
    const attempts = screen.getByText(/Attempts \(3\)/);
    expect(attempts).toBeInTheDocument();
    const nullProbeRow = screen.getByText('bench_01').closest('tr')!;
    // endToEnd present; peak, swap, thermal, and the empty evidence
    // cell are all — (and no stray "0 B" from a null probe).
    expect(within(nullProbeRow).getAllByText('—')).toHaveLength(4);
    expect(within(nullProbeRow).queryByText('0 B')).not.toBeInTheDocument();
    const probedRow = screen.getByText('bench_02').closest('tr')!;
    expect(within(probedRow).getByText('2.0 GB')).toBeInTheDocument();
    expect(within(probedRow).getByText('−1 KB')).toBeInTheDocument();
    expect(within(probedRow).getByText('nominal → nominal')).toBeInTheDocument();
  });

  it('lists failed attempts and invalid lines under failures', async () => {
    const failed = recordLine((r) => {
      r.outcome.status = 'failed';
      r.outcome.finalTaskSuccess = false;
      r.outcome.errorClass = 'oracle-mismatch';
    });
    installTree({
      'benchmark-artifacts/run/records.jsonl': `${failed}\nnot json at all\n`,
    });
    render(<BenchmarksPanel />);
    expect(await screen.findByText(/Failures & refusals/)).toBeInTheDocument();
    expect(screen.getByText(/failed \(oracle-mismatch\)/)).toBeInTheDocument();
    expect(screen.getByText(/invalid record — line 2/)).toBeInTheDocument();
  });

  it('shows the walk-budget refusal as a visible alert, not an empty view', async () => {
    const tree: Record<string, string> = {};
    for (let index = 0; index < 65; index += 1) {
      tree[`benchmark-artifacts/run/records-${index}.jsonl`] = recordLine();
    }
    installTree(tree);
    render(<BenchmarksPanel />);
    const alert = await screen.findByText(/Results refused:/);
    expect(alert).toHaveTextContent(/refusing to render an arbitrary subset/);
  });

  it('shows a refused file with its reason instead of silently dropping it', async () => {
    installTree({ 'benchmark-artifacts/run/records.jsonl': 'x' });
    mocks.readFile.mockResolvedValue({ content: '', encoding: 'binary', bytes: 4 });
    render(<BenchmarksPanel />);
    expect(await screen.findByText(/File refused: Not a UTF-8 text file\./)).toBeInTheDocument();
  });

  it('renders the catalog tables and refuses a broken catalog visibly', async () => {
    installTree({
      'benchmarks/catalog/models.json': JSON.stringify({
        schemaVersion: 1,
        models: [
          {
            id: 'fake-model',
            displayName: 'Fake Model',
            folder: 'Fake-Folder',
            engine: 'mlx-lm',
            maxContextTokens: 8192,
            artifact: {
              format: 'mlx',
              sha256: `sha256:${'ab'.repeat(32)}`,
              quantizationMethod: 'affine',
              quantizationBits: 4,
              quantizationGroupSize: 64,
            },
          },
        ],
      }),
      'benchmarks/catalog/presets.json': JSON.stringify({
        schemaVersion: 1,
        presets: [
          {
            id: 'fake-preset',
            description: 'A fake preset for viewer tests',
            model: 'fake-model',
            measurementPaths: ['rawRuntime'],
            generation: {
              temperature: 0,
              topP: 1,
              topK: null,
              minP: null,
              repeatPenalty: null,
              seed: 42,
              maxOutputTokens: 64,
              stopSequences: [],
            },
            contextTokens: 4096,
            suites: [{ fixture: 'short-chat/pong-001', populations: ['warm'], repetitions: 3 }],
          },
        ],
      }),
      'benchmarks/fixtures/short-chat/pong-001/manifest.json': '{}',
    });
    render(<BenchmarksPanel />);
    expect(await screen.findByText('Fake Model')).toBeInTheDocument();
    expect(screen.getByText('fake-preset')).toBeInTheDocument();
    expect(screen.getByText('affine, 4-bit, group 64')).toBeInTheDocument();
  });

  it('opens an evidence preview from an attempt link and closes it', async () => {
    const user = userEvent.setup();
    const withEvidence = recordLine((r) => {
      r.artifacts = ['benchmark-artifacts/run/attempt-1.txt'];
    });
    installTree({
      'benchmark-artifacts/run/records.jsonl': withEvidence,
      'benchmark-artifacts/run/attempt-1.txt': 'raw evidence bytes',
    });
    render(<BenchmarksPanel />);
    const link = await screen.findByRole('button', { name: 'attempt-1.txt' });
    await user.click(link);
    const dialog = await screen.findByRole('dialog', {
      name: /attempt-1\.txt/,
    });
    expect(await within(dialog).findByText('raw evidence bytes')).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });
  });

  it('wraps the attempts table in a horizontal scroll owner', async () => {
    // Codex's constrained-window packaged finding: the nine-column
    // attempts table rendered with no overflow owner, so narrow
    // windows clipped Status and made the resource/evidence columns
    // unreachable. Pin the wrapper itself, not just font/border rules.
    installTree({ 'benchmark-artifacts/run/records.jsonl': recordLine() });
    render(<BenchmarksPanel />);
    const summary = await screen.findByText(/Attempts \(1\)/);
    const details = summary.closest('details')!;
    const table = details.querySelector('table.plume-benchmarks-table')!;
    expect(table.parentElement?.className).toBe('plume-benchmarks-table-scroll');
    // Keyboard-reachable scroll: the container is focusable.
    expect(table.parentElement?.getAttribute('tabindex')).toBe('0');
  });

  it('reloads on Refresh', async () => {
    installTree({});
    render(<BenchmarksPanel />);
    await screen.findByText(/no benchmark evidence yet/i);
    installTree({ 'benchmark-artifacts/run/records.jsonl': recordLine() });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByText(/HARNESS TEST DATA/)).toBeInTheDocument();
  });
});
