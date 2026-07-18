import { useEffect, useRef } from 'react';

import { QWEN_CATALOG_ID } from '../../lib/api/providers';
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
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const wasOpenRef = useRef(open);
  const apple = catalog.entry('apple-system');
  const qwen = catalog.entry(QWEN_CATALOG_ID);

  useEffect(() => {
    if (open) dialogRef.current?.focus();
    if (!open && wasOpenRef.current) triggerRef.current?.focus();
    wasOpenRef.current = open;
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      onOpenChange(false);
    };
    const onPointerDown = (event: PointerEvent) => {
      if (anchorRef.current?.contains(event.target as Node)) return;
      onOpenChange(false);
    };
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('pointerdown', onPointerDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('pointerdown', onPointerDown);
    };
  }, [onOpenChange, open]);

  return (
    <div className="plume-model-chooser-anchor" ref={anchorRef}>
      <button
        ref={triggerRef}
        type="button"
        className="ink-button plume-model-chooser-trigger"
        data-tauri-drag-region="false"
        aria-label="Model"
        aria-describedby="plume-model-chooser-value"
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => onOpenChange(!open)}
      >
        <span className="plume-model-chooser-trigger-label" aria-hidden="true">Model</span>
        <span id="plume-model-chooser-value" className="plume-model-chooser-trigger-value">{selectionLabel(selection, apple, qwen)}</span>
      </button>
      {open ? (
        <div
          ref={dialogRef}
          className="plume-model-chooser-popover"
          role="dialog"
          aria-label="Choose a model"
          tabIndex={-1}
        >
          <div className="plume-model-chooser-heading">
            <h3>Choose a model</h3>
            <p>Pick one to start chatting.</p>
          </div>
          <div className="plume-model-chooser-cards">
            <AppleCard
              entry={apple}
              catalog={catalog}
              selection={selection}
              onDone={() => onOpenChange(false)}
            />
            <QwenCard entry={qwen} catalog={catalog} selection={selection} onDone={() => onOpenChange(false)} />
          </div>
        </div>
      ) : null}
    </div>
  );
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
    return (
      <section className="plume-model-chooser-card" aria-labelledby="plume-apple-model-title">
        <div>
          <h4 id="plume-apple-model-title">Apple On-Device</h4>
          <p>Built into this Mac</p>
        </div>
        <p className="plume-model-chooser-status" role="status">
          {retry ? 'Couldn’t load models.' : 'Checking model status…'}
        </p>
        <button
          type="button"
          className="ink-button plume-model-chooser-action"
          disabled={!retry}
          onClick={() => void catalog.refresh()}
        >
          {retry ? 'Try again' : 'Checking'}
        </button>
        <ModelDetails entry={null} error={catalog.error} />
      </section>
    );
  }
  const available = entry?.state === 'available';
  const selected = selection.selected?.providerId === 'apple-foundation';
  const reason = entry?.availabilityReason ?? 'Checking whether this Mac can use Apple’s model.';
  const reasonId = 'plume-apple-model-reason';
  return (
    <section className="plume-model-chooser-card" aria-labelledby="plume-apple-model-title">
      <div>
        <h4 id="plume-apple-model-title">Apple On-Device</h4>
        <p>Built into this Mac</p>
      </div>
      {available ? null : <p id={reasonId} className="plume-model-chooser-status" role="status">{reason}</p>}
      {selected ? (
        <span className="ink-badge plume-model-chooser-selected" aria-label="Apple model is selected">Selected</span>
      ) : (
        <button
          type="button"
          className="ink-button plume-model-chooser-action"
          disabled={!available}
          aria-describedby={available ? undefined : reasonId}
          onClick={() => void closeAfterSelection(catalog.useApple, selection, onDone)}
        >
          Use Apple Model
        </button>
      )}
      <ModelDetails entry={entry} />
    </section>
  );
}

function QwenCard({
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
  const retry = entry === null && !catalog.loading;
  const state = entry?.state ?? 'checking';
  const isSelected = selection.selected?.modelId === QWEN_CATALOG_ID;
  const action = retry
    ? <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void catalog.refresh()}>Try again</button>
    : qwenAction(state, entry, catalog, selection, isSelected, onDone);
  return (
    <section className="plume-model-chooser-card" aria-labelledby="plume-qwen-model-title">
      <div>
        <h4 id="plume-qwen-model-title">Qwen Coder 1.5B</h4>
        <p>Recommended for coding</p>
      </div>
      {state === 'downloading' ? <DownloadProgress entry={entry} /> : null}
      {state === 'verifying' ? <p className="plume-model-chooser-status" role="status">Verifying download…</p> : null}
      {state === 'failed' ? <p className="plume-model-chooser-status plume-model-chooser-error" role="status">Couldn’t finish the download. Try again.</p> : null}
      {state === 'checking' ? <p className="plume-model-chooser-status" role="status">{retry ? 'Couldn’t load models.' : 'Checking model status…'}</p> : null}
      {action}
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
  onDone: () => void,
) {
  if (state === 'downloading') {
    return <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void catalog.cancelDownload(QWEN_CATALOG_ID)}>Cancel</button>;
  }
  if (state === 'verifying' || state === 'checking') {
    return <button type="button" className="ink-button plume-model-chooser-action" disabled>{state === 'checking' ? 'Checking' : 'Verifying'}</button>;
  }
  if (state === 'failed') {
    return <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void catalog.download(QWEN_CATALOG_ID)}>Retry</button>;
  }
  if (state === 'absent') {
    return <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void catalog.download(QWEN_CATALOG_ID)}>Download {formatMegabytes(entry?.downloadBytes)}</button>;
  }
  if (state === 'running' && isSelected) {
    return <span className="ink-badge plume-model-chooser-selected" aria-label="Qwen is selected">Selected</span>;
  }
  return <button type="button" className="ink-button plume-model-chooser-action" onClick={() => void closeAfterSelection(catalog.useQwen, selection, onDone)}>Use Qwen</button>;
}

function DownloadProgress({ entry }: { entry: ModelCatalogEntry | null }) {
  const total = entry?.totalBytes ?? entry?.downloadBytes ?? 0;
  const downloaded = entry?.downloadedBytes ?? 0;
  const percent = total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
  return (
    <div className="plume-model-chooser-progress">
      <progress
        aria-label="Downloading Qwen Coder"
        aria-valuenow={percent}
        value={percent}
        max="100"
      >
        {percent}%
      </progress>
      <span>Downloading Qwen Coder · {percent}%</span>
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
): string {
  if (selection.selected === null) return 'Choose model';
  if (selection.selected.providerId === 'apple-foundation') return apple?.displayName ?? 'Apple On-Device';
  if (selection.selected.modelId === QWEN_CATALOG_ID) return qwen?.displayName ?? 'Qwen Coder 1.5B';
  return selection.selected.providerDisplayName;
}

function formatMegabytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || bytes <= 0) return 'model';
  return `${Math.round(bytes / 1_000_000)} MB`;
}
