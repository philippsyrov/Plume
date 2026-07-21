import { useEffect, useRef } from 'react';

import { QWEN_CATALOG_ID, QWEN_VISION_CATALOG_ID } from '../../lib/api/providers';
import type { ModelCatalogApi, ModelCatalogEntry } from './useModelCatalog';
import type { SelectedModelApi } from './useSelectedModel';

export function ModelChooser({
  open,
  onOpenChange,
  catalog,
  selection,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  catalog: ModelCatalogApi;
  selection: SelectedModelApi;
}) {
  return (
    <div className="plume-model-chooser-anchor">
      <ModelChooserTrigger
        open={open}
        onOpenChange={onOpenChange}
        catalog={catalog}
        selection={selection}
      />
      {open ? (
        <ModelChooserWorkspace
          catalog={catalog}
          selection={selection}
          onClose={() => onOpenChange(false)}
        />
      ) : null}
    </div>
  );
}

export function ModelChooserTrigger({
  open,
  onOpenChange,
  catalog,
  selection,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  catalog: ModelCatalogApi;
  selection: SelectedModelApi;
}) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const wasOpenRef = useRef(open);
  const apple = catalog.entry('apple-system');
  const qwen = catalog.entry(QWEN_CATALOG_ID);
  const qwenVision = catalog.entry(QWEN_VISION_CATALOG_ID);

  useEffect(() => {
    if (!open && wasOpenRef.current) triggerRef.current?.focus();
    wasOpenRef.current = open;
  }, [open]);

  return (
    <button
      ref={triggerRef}
      type="button"
      className="ink-button plume-model-chooser-trigger"
      data-tauri-drag-region="false"
      aria-label="Model"
      aria-describedby="plume-model-chooser-value"
      aria-expanded={open}
      onClick={() => onOpenChange(!open)}
    >
      <span id="plume-model-chooser-value" className="plume-model-chooser-trigger-value">
        {selectionLabel(selection, apple, qwen, qwenVision)}
      </span>
    </button>
  );
}

export function ModelChooserWorkspace({
  catalog,
  selection,
  onClose,
}: {
  catalog: ModelCatalogApi;
  selection: SelectedModelApi;
  onClose: () => void;
}) {
  const workspaceRef = useRef<HTMLElement | null>(null);
  const apple = catalog.entry('apple-system');
  const qwen = catalog.entry(QWEN_CATALOG_ID);
  const qwenVision = catalog.entry(QWEN_VISION_CATALOG_ID);

  useEffect(() => {
    workspaceRef.current?.focus();
  }, []);

  return (
    <section
      ref={workspaceRef}
      className="plume-model-chooser-workspace"
      role="region"
      aria-label="Choose a model"
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key !== 'Escape') return;
        event.preventDefault();
        onClose();
      }}
    >
      <div className="plume-model-chooser-cards">
        <AppleCard
          entry={apple}
          catalog={catalog}
          selection={selection}
          onDone={onClose}
        />
        <QwenCard
          entry={qwen}
          catalog={catalog}
          selection={selection}
          peerDownloadActive={isDownloadActive(qwenVision)}
          peerStarting={isStarting(qwenVision)}
          onDone={onClose}
        />
        <ManagedModelCard
          entry={qwenVision}
          catalog={catalog}
          selection={selection}
          catalogId={QWEN_VISION_CATALOG_ID}
          title={qwenVision?.displayName ?? 'Qwen2-VL 2B'}
          subtitle={qwenVision?.subtitle ?? 'Understands images'}
          onUse={catalog.useQwenVision}
          peerDownloadActive={isDownloadActive(qwen)}
          peerStarting={isStarting(qwen)}
          onDone={onClose}
        />
      </div>
    </section>
  );
}

function ManagedModelCard({
  entry,
  catalog,
  selection,
  catalogId,
  title,
  subtitle,
  onUse,
  peerDownloadActive,
  peerStarting,
  onDone,
}: {
  entry: ModelCatalogEntry | null;
  catalog: ModelCatalogApi;
  selection: SelectedModelApi;
  catalogId: typeof QWEN_VISION_CATALOG_ID;
  title: string;
  subtitle: string;
  onUse: () => Promise<void>;
  peerDownloadActive: boolean;
  peerStarting: boolean;
  onDone: () => void;
}) {
  const retry = entry === null && !catalog.loading;
  const state = entry?.state ?? 'checking';
  const selected = selection.selected?.modelId === catalogId;
  const label = title.replace(' 2B', '');
  const peerDownloadBlocked = peerDownloadActive && (state === 'absent' || state === 'failed');
  const peerStartBlocked = peerStarting && !selected && (state === 'installed' || state === 'running');
  const action = retry ? (
    <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void catalog.refresh()}>Try again</button>
  ) : managedAction(
    state,
    entry,
    catalog,
    catalogId,
    label,
    selected,
    onUse,
    selection,
    peerDownloadActive,
    peerStarting,
    onDone,
  );
  return (
    <section className="plume-model-chooser-row" role="group" aria-labelledby="plume-qwen-vision-model-title">
      <div className="plume-model-chooser-row-main">
        <div className="plume-model-chooser-copy">
          <h4 id="plume-qwen-vision-model-title">{title}</h4>
          <p>{subtitle}</p>
        </div>
        <div className="plume-model-chooser-row-action">{action}</div>
      </div>
      {state === 'downloading' ? <DownloadProgress entry={entry} label={label} /> : null}
      {state === 'verifying' ? <p className="plume-model-chooser-status" role="status">Verifying download…</p> : null}
      {state === 'failed' && !peerDownloadBlocked ? <p className="plume-model-chooser-status plume-model-chooser-error" role="status">Couldn’t finish the download. Try again.</p> : null}
      {peerDownloadBlocked ? <p className="plume-model-chooser-status" role="status">Finish or cancel the other download first.</p> : null}
      {peerStartBlocked ? <p className="plume-model-chooser-status" role="status">Wait for the other model to finish starting.</p> : null}
      {state === 'starting' ? <p className="plume-model-chooser-status" role="status">Starting {label}…</p> : null}
      {state === 'start-failed' ? <p className="plume-model-chooser-status plume-model-chooser-error" role="status">Couldn’t start {label}. Try again.</p> : null}
      {state === 'checking' ? <p className="plume-model-chooser-status" role="status">{retry ? 'Couldn’t load models.' : 'Checking model status…'}</p> : null}
      <ModelDetails entry={entry} {...(entry === null && catalog.error !== null ? { error: catalog.error } : {})} />
    </section>
  );
}

function managedAction(
  state: string,
  entry: ModelCatalogEntry | null,
  catalog: ModelCatalogApi,
  catalogId: typeof QWEN_VISION_CATALOG_ID,
  label: string,
  selected: boolean,
  onUse: () => Promise<void>,
  selection: SelectedModelApi,
  peerDownloadActive: boolean,
  peerStarting: boolean,
  onDone: () => void,
) {
  if (state === 'downloading') return <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void catalog.cancelDownload(catalogId)}>Cancel</button>;
  if (state === 'verifying' || state === 'checking' || state === 'starting') {
    const actionLabel = state === 'checking' ? 'Checking' : state === 'verifying' ? 'Verifying' : 'Starting';
    return <button type="button" className="ink-button plume-model-chooser-action" disabled>{actionLabel}</button>;
  }
  if (state === 'failed') return <button type="button" className="ink-button plume-model-chooser-action" disabled={peerDownloadActive} onClick={() => void catalog.download(catalogId)}>Retry</button>;
  if (state === 'start-failed') return <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void closeAfterSelection(onUse, selection, onDone)}>Retry</button>;
  if (state === 'absent') return <button type="button" className="ink-button plume-model-chooser-action" disabled={peerDownloadActive} onClick={() => void catalog.download(catalogId)}>Download {formatDownloadSize(entry?.downloadBytes)}</button>;
  if (state === 'running' && selected) return <span className="ink-badge plume-model-chooser-selected" aria-label={`${label} is selected`}>Selected</span>;
  return <button type="button" className="ink-button plume-model-chooser-action" disabled={peerStarting} onClick={() => void closeAfterSelection(onUse, selection, onDone)}>Use {label}</button>;
}

function AppleCard({
  entry,
  catalog,
  selection,
  onDone,
}: {
  entry: ModelCatalogEntry | null;
  catalog: ModelCatalogApi;
  selection: SelectedModelApi;
  onDone: () => void;
}) {
  if (entry === null) {
    const retry = !catalog.loading;
    const action = (
      <button
        type="button"
        className="ink-button plume-model-chooser-action"
        disabled={!retry}
        onClick={() => void catalog.refresh()}
      >
        {retry ? 'Try again' : 'Checking'}
      </button>
    );
    return (
      <section className="plume-model-chooser-row" role="group" aria-labelledby="plume-apple-model-title">
        <div className="plume-model-chooser-row-main">
          <div className="plume-model-chooser-copy">
            <h4 id="plume-apple-model-title">Apple On-Device</h4>
            <p>Built into this Mac</p>
          </div>
          <div className="plume-model-chooser-row-action">{action}</div>
        </div>
        <p className="plume-model-chooser-status" role="status">
          {retry ? 'Couldn’t load models.' : 'Checking model status…'}
        </p>
        <ModelDetails entry={null} error={catalog.error} />
      </section>
    );
  }
  const available = entry?.state === 'available';
  const selected = selection.selected?.providerId === 'apple-foundation';
  const reason = entry?.availabilityReason ?? 'Checking whether this Mac can use Apple’s model.';
  const reasonId = 'plume-apple-model-reason';
  const action = selected ? (
    <span className="ink-badge plume-model-chooser-selected" aria-label="Apple model is selected">Selected</span>
  ) : (
    <button
      type="button"
      className="ink-button plume-model-chooser-action"
      disabled={!available}
      aria-describedby={available ? undefined : reasonId}
      aria-label="Use Apple Model"
      onClick={() => void closeAfterSelection(catalog.useApple, selection, onDone)}
    >
      Use Apple
    </button>
  );
  return (
    <section className="plume-model-chooser-row" role="group" aria-labelledby="plume-apple-model-title">
      <div className="plume-model-chooser-row-main">
        <div className="plume-model-chooser-copy">
          <h4 id="plume-apple-model-title">Apple On-Device</h4>
          <p>Built into this Mac</p>
        </div>
        <div className="plume-model-chooser-row-action">{action}</div>
      </div>
      {available ? null : <p id={reasonId} className="plume-model-chooser-status" role="status">{reason}</p>}
      <ModelDetails entry={entry} />
    </section>
  );
}

function QwenCard({
  entry,
  catalog,
  selection,
  peerDownloadActive,
  peerStarting,
  onDone,
}: {
  entry: ModelCatalogEntry | null;
  catalog: ModelCatalogApi;
  selection: SelectedModelApi;
  peerDownloadActive: boolean;
  peerStarting: boolean;
  onDone: () => void;
}) {
  const retry = entry === null && !catalog.loading;
  const state = entry?.state ?? 'checking';
  const isSelected = selection.selected?.modelId === QWEN_CATALOG_ID;
  const peerDownloadBlocked = peerDownloadActive && (state === 'absent' || state === 'failed');
  const peerStartBlocked = peerStarting && !isSelected && (state === 'installed' || state === 'running');
  const action = retry
    ? <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void catalog.refresh()}>Try again</button>
    : qwenAction(
      state,
      entry,
      catalog,
      selection,
      isSelected,
      peerDownloadActive,
      peerStarting,
      onDone,
    );
  return (
    <section className="plume-model-chooser-row" role="group" aria-labelledby="plume-qwen-model-title">
      <div className="plume-model-chooser-row-main">
        <div className="plume-model-chooser-copy">
          <h4 id="plume-qwen-model-title">Qwen Coder 1.5B</h4>
          <p>Recommended for coding</p>
        </div>
        <div className="plume-model-chooser-row-action">{action}</div>
      </div>
      {state === 'downloading' ? <DownloadProgress entry={entry} /> : null}
      {state === 'verifying' ? <p className="plume-model-chooser-status" role="status">Verifying download…</p> : null}
      {state === 'failed' && !peerDownloadBlocked ? <p className="plume-model-chooser-status plume-model-chooser-error" role="status">Couldn’t finish the download. Try again.</p> : null}
      {peerDownloadBlocked ? <p className="plume-model-chooser-status" role="status">Finish or cancel the other download first.</p> : null}
      {peerStartBlocked ? <p className="plume-model-chooser-status" role="status">Wait for the other model to finish starting.</p> : null}
      {state === 'starting' ? <p className="plume-model-chooser-status" role="status">Starting Qwen…</p> : null}
      {state === 'start-failed' ? <p className="plume-model-chooser-status plume-model-chooser-error" role="status">Couldn’t start Qwen. Try again.</p> : null}
      {state === 'checking' ? <p className="plume-model-chooser-status" role="status">{retry ? 'Couldn’t load models.' : 'Checking model status…'}</p> : null}
      <ModelDetails
        entry={entry}
        {...(entry === null && catalog.error !== null ? { error: catalog.error } : {})}
      />
    </section>
  );
}

function qwenAction(
  state: string,
  entry: ModelCatalogEntry | null,
  catalog: ModelCatalogApi,
  selection: SelectedModelApi,
  isSelected: boolean,
  peerDownloadActive: boolean,
  peerStarting: boolean,
  onDone: () => void,
) {
  if (state === 'downloading') {
    return <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void catalog.cancelDownload(QWEN_CATALOG_ID)}>Cancel</button>;
  }
  if (state === 'verifying' || state === 'checking' || state === 'starting') {
    const label = state === 'checking' ? 'Checking' : state === 'verifying' ? 'Verifying' : 'Starting';
    return <button type="button" className="ink-button plume-model-chooser-action" disabled>{label}</button>;
  }
  if (state === 'failed') {
    return <button type="button" className="ink-button plume-model-chooser-action" disabled={peerDownloadActive} onClick={() => void catalog.download(QWEN_CATALOG_ID)}>Retry</button>;
  }
  if (state === 'start-failed') {
    return <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void closeAfterSelection(catalog.useQwen, selection, onDone)}>Retry</button>;
  }
  if (state === 'absent') {
    return <button type="button" className="ink-button plume-model-chooser-action" disabled={peerDownloadActive} onClick={() => void catalog.download(QWEN_CATALOG_ID)}>Download {formatDownloadSize(entry?.downloadBytes)}</button>;
  }
  if (state === 'running' && isSelected) {
    return <span className="ink-badge plume-model-chooser-selected" aria-label="Qwen is selected">Selected</span>;
  }
  return <button type="button" className="ink-button plume-model-chooser-action" disabled={peerStarting} onClick={() => void closeAfterSelection(catalog.useQwen, selection, onDone)}>Use Qwen</button>;
}

function isDownloadActive(entry: ModelCatalogEntry | null): boolean {
  return entry?.state === 'downloading' || entry?.state === 'verifying';
}

function isStarting(entry: ModelCatalogEntry | null): boolean {
  return entry?.state === 'starting';
}

function DownloadProgress({ entry, label = 'Qwen Coder' }: { entry: ModelCatalogEntry | null; label?: string }) {
  const total = entry?.totalBytes ?? entry?.downloadBytes ?? 0;
  const downloaded = entry?.downloadedBytes ?? 0;
  const percent = total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
  return (
    <div className="plume-model-chooser-progress">
      <progress
        aria-label={`Downloading ${label}`}
        aria-valuenow={percent}
        value={percent}
        max="100"
      >
        {percent}%
      </progress>
      <span>Downloading {label} · {formatBytes(downloaded)} of {formatBytes(total)} ({percent}%)</span>
    </div>
  );
}

function ModelDetails({ entry, error: fallbackError }: { entry: ModelCatalogEntry | null; error?: string | null }) {
  const source = entry?.sourceUrl;
  const license = entry?.license;
  const error = entry?.error ?? fallbackError;
  if (!source && !license && !error) return null;
  return (
    <details className="plume-model-chooser-details">
      <summary>Details</summary>
      {source ? <p>Source: {source}</p> : null}
      {license ? <p>License: {license}</p> : null}
      {error ? <p>Error: {error}</p> : null}
    </details>
  );
}

async function closeAfterSelection(
  action: () => Promise<void>,
  selection: SelectedModelApi,
  onDone: () => void,
): Promise<void> {
  const before = selection.revision();
  await action();
  if (selection.revision() !== before) onDone();
}

function selectionLabel(
  selection: SelectedModelApi,
  apple: ModelCatalogEntry | null,
  qwen: ModelCatalogEntry | null,
  qwenVision: ModelCatalogEntry | null,
): string {
  if (selection.selected === null) return 'Choose model';
  if (selection.selected.providerId === 'apple-foundation') return apple?.displayName ?? 'Apple On-Device';
  if (selection.selected.modelId === QWEN_CATALOG_ID) return qwen?.displayName ?? 'Qwen Coder 1.5B';
  if (selection.selected.modelId === QWEN_VISION_CATALOG_ID) return qwenVision?.displayName ?? 'Qwen2-VL 2B';
  return selection.selected.providerDisplayName;
}

function formatDownloadSize(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || bytes <= 0) return 'model';
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  return `${Math.round(bytes / 1_000_000)} MB`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1_000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1_000)} KB`;
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  return `${Math.round(bytes / 1_000_000)} MB`;
}
