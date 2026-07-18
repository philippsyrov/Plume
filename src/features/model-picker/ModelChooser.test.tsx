import { useEffect, useRef, useState } from 'react';
import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { ModelCatalogApi, ModelCatalogEntry } from './useModelCatalog';
import type { SelectedModelApi } from './useSelectedModel';
import { ModelChooser } from './ModelChooser';

function entry(overrides: Partial<ModelCatalogEntry>): ModelCatalogEntry {
  return {
    id: 'apple-system',
    displayName: 'Apple On-Device',
    subtitle: 'Built into this Mac',
    providerId: 'apple-foundation',
    modelId: 'system',
    state: 'available',
    availabilityReason: null,
    downloadBytes: null,
    license: 'Apple terms',
    sourceUrl: 'https://apple.example/model',
    revision: null,
    operationId: null,
    downloadedBytes: null,
    totalBytes: null,
    error: null,
    ...overrides,
  };
}

function renderChooser({
  selected = null,
  apple = entry({}),
  qwen = entry({
    id: 'qwen-coder-1.5b-mlx-4bit',
    displayName: 'Qwen Coder 1.5B',
    subtitle: 'Recommended for coding',
    providerId: 'mlx-lm',
    modelId: 'qwen-coder-1.5b-mlx-4bit',
    state: 'absent',
    downloadBytes: 868_628_559,
    license: 'Apache-2.0',
    sourceUrl: 'https://huggingface.co/mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit',
  }),
  open = false,
}: {
  selected?: SelectedModelApi['selected'];
  apple?: ModelCatalogEntry;
  qwen?: ModelCatalogEntry;
  open?: boolean;
} = {}) {
  const onOpenChange = vi.fn();
  const catalog: ModelCatalogApi = {
    entries: [apple, qwen],
    entry: (id) => [apple, qwen].find((candidate) => candidate.id === id) ?? null,
    loading: false,
    downloadEventsReady: true,
    error: null,
    download: vi.fn().mockResolvedValue(undefined),
    cancelDownload: vi.fn().mockResolvedValue(undefined),
    useApple: vi.fn().mockResolvedValue(undefined),
    useQwen: vi.fn().mockResolvedValue(undefined),
    removeQwen: vi.fn().mockResolvedValue(undefined),
    refresh: vi.fn().mockResolvedValue(undefined),
  };
  const selection: SelectedModelApi = {
    selected,
    select: vi.fn(),
    clear: vi.fn(),
    revision: () => 0,
  };
  render(
    <ModelChooser
      open={open}
      onOpenChange={onOpenChange}
      catalog={catalog}
      selection={selection}
    />,
  );
  return { catalog, onOpenChange };
}

describe('ModelChooser', () => {
  it('keeps a stable Model name while exposing the selected value', async () => {
    const { onOpenChange } = renderChooser();
    const trigger = screen.getByRole('button', { name: 'Model' });
    expect(trigger).toHaveTextContent('Choose model');
    expect(trigger).toHaveAccessibleDescription('Choose model');

    await userEvent.click(trigger);
    expect(onOpenChange).toHaveBeenCalledWith(true);
  });

  it('renders the two consumer-facing cards without technical primary copy', () => {
    renderChooser({ open: true });

    expect(screen.getByRole('heading', { name: 'Apple On-Device' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Qwen Coder 1.5B' })).toBeVisible();
    expect(screen.getByRole('button', { name: /Download.*869 MB/ })).toBeVisible();
    expect(screen.queryByText(/\/Users\//)).toBeNull();
    expect(screen.queryByText(/port|pid/i)).toBeNull();
  });

  it('renders two compact model rows instead of nested model cards', () => {
    renderChooser({ open: true });

    const dialog = screen.getByRole('dialog', { name: 'Choose a model' });
    const rows = dialog.querySelectorAll<HTMLElement>('.plume-model-chooser-row');
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveAccessibleName('Apple On-Device');
    expect(rows[1]).toHaveAccessibleName('Qwen Coder 1.5B');
    expect(dialog.querySelectorAll('.plume-model-chooser-card')).toHaveLength(0);
  });

  it('contains forward and backward Tab focus while open', async () => {
    render(<ControlledChooser />);
    await userEvent.click(screen.getByRole('button', { name: 'Model' }));

    const dialog = screen.getByRole('dialog', { name: 'Choose a model' });
    const apple = within(dialog).getByRole('button', { name: 'Use Apple Model' });
    const lastDetails = within(dialog).getAllByText('Details').at(-1)!;

    lastDetails.focus();
    await userEvent.keyboard('{Tab}');
    expect(apple).toHaveFocus();

    apple.focus();
    await userEvent.keyboard('{Shift>}{Tab}{/Shift}');
    expect(lastDetails).toHaveFocus();
  });

  it('contains Tab after a deferred availability transition disables the focused action', async () => {
    const transition = deferred<void>();
    render(<DeferredAvailabilityChooser transition={transition.promise} />);
    await userEvent.click(screen.getByRole('button', { name: 'Model' }));

    const dialog = screen.getByRole('dialog', { name: 'Choose a model' });
    const apple = within(dialog).getByRole('button', { name: 'Use Apple Model' });
    const firstDetails = within(dialog).getAllByText('Details')[0]!;
    apple.focus();

    await act(async () => {
      transition.resolve();
      await transition.promise;
    });

    expect(apple).toBeDisabled();
    expect(apple).toHaveFocus();
    await userEvent.keyboard('{Tab}');
    expect(firstDetails).toHaveFocus();
  });

  it('shows accessible download progress and lets the user cancel', async () => {
    const { catalog } = renderChooser({
      open: true,
      qwen: entry({
        id: 'qwen-coder-1.5b-mlx-4bit',
        displayName: 'Qwen Coder 1.5B',
        subtitle: 'Recommended for coding',
        providerId: 'mlx-lm',
        modelId: 'qwen-coder-1.5b-mlx-4bit',
        state: 'downloading',
        downloadBytes: 1000,
        downloadedBytes: 100,
        totalBytes: 1000,
      }),
    });

    expect(screen.getByRole('progressbar', { name: 'Downloading Qwen Coder' }))
      .toHaveAttribute('aria-valuenow', '10');
    expect(screen.getByText('Downloading Qwen Coder · 100 B of 1 KB (10%)')).toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(catalog.cancelDownload).toHaveBeenCalledWith('qwen-coder-1.5b-mlx-4bit');
  });

  it('announces managed Qwen startup and prevents a second activation', () => {
    renderChooser({
      open: true,
      qwen: entry({
        id: 'qwen-coder-1.5b-mlx-4bit', displayName: 'Qwen Coder 1.5B',
        subtitle: 'Recommended for coding', providerId: 'mlx-lm',
        modelId: 'qwen-coder-1.5b-mlx-4bit', state: 'starting', downloadBytes: 868_628_559,
      }),
    });

    expect(screen.getByText('Starting Qwen…')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Starting' })).toBeDisabled();
  });

  it('retries a failed managed start without restarting the download', async () => {
    const { catalog } = renderChooser({
      open: true,
      qwen: entry({
        id: 'qwen-coder-1.5b-mlx-4bit', displayName: 'Qwen Coder 1.5B',
        subtitle: 'Recommended for coding', providerId: 'mlx-lm',
        modelId: 'qwen-coder-1.5b-mlx-4bit', state: 'start-failed', downloadBytes: 868_628_559,
        error: 'managed process failed at /private/tmp/qwen.log',
      }),
    });

    expect(screen.getByText('Couldn’t start Qwen. Try again.')).toBeVisible();
    expect(screen.getByText(/Error: managed process failed at \/private\/tmp\/qwen\.log/)).not.toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(catalog.useQwen).toHaveBeenCalledOnce();
    expect(catalog.download).not.toHaveBeenCalled();
  });

  it('keeps Apple’s unavailable reason short and offers Qwen retry without exposing its raw error', async () => {
    const { catalog } = renderChooser({
      open: true,
      apple: entry({ state: 'unavailable', availabilityReason: 'Apple Intelligence is turned off.' }),
      qwen: entry({
        id: 'qwen-coder-1.5b-mlx-4bit', displayName: 'Qwen Coder 1.5B',
        subtitle: 'Recommended for coding', providerId: 'mlx-lm',
        modelId: 'qwen-coder-1.5b-mlx-4bit', state: 'failed', downloadBytes: 868_628_559,
        error: 'download failed at /private/tmp/qwen.log',
      }),
    });

    expect(screen.getByRole('button', { name: 'Use Apple Model' })).toBeDisabled();
    expect(screen.getByText('Apple Intelligence is turned off.')).toBeVisible();
    expect(screen.getByText('Couldn’t finish the download. Try again.')).toBeVisible();
    expect(screen.getByText(/Error: download failed at \/private\/tmp\/qwen\.log/)).not.toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(catalog.download).toHaveBeenCalledWith('qwen-coder-1.5b-mlx-4bit');
  });

  it('offers a recovery action when the catalog could not load', async () => {
    const catalog = catalogFor(entry({}), entry({
      id: 'qwen-coder-1.5b-mlx-4bit', displayName: 'Qwen Coder 1.5B',
      subtitle: 'Recommended for coding', providerId: 'mlx-lm',
      modelId: 'qwen-coder-1.5b-mlx-4bit', state: 'absent', downloadBytes: 868_628_559,
    }), { loading: false, error: 'catalog IPC failed at /private/tmp/catalog.log', entry: () => null });
    render(
      <ModelChooser
        open
        onOpenChange={vi.fn()}
        catalog={catalog}
        selection={emptySelection()}
      />,
    );

    const retry = screen.getAllByRole('button', { name: 'Try again' })[0]!;
    await userEvent.click(retry);
    expect(catalog.refresh).toHaveBeenCalled();
    expect(screen.getAllByText('Couldn’t load models.')).toHaveLength(2);
  });

  it('closes on Escape and returns focus to the trigger', async () => {
    render(<ControlledChooser />);
    const trigger = screen.getByRole('button', { name: 'Model' });

    await userEvent.click(trigger);
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog', { name: 'Choose a model' })).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it('closes when the user presses outside the controlled popover', async () => {
    render(<ControlledChooser />);
    await userEvent.click(screen.getByRole('button', { name: 'Model' }));
    expect(screen.getByRole('dialog', { name: 'Choose a model' })).toBeVisible();

    await userEvent.pointer({ target: document.body, keys: '[MouseLeft]' });
    expect(screen.queryByRole('dialog', { name: 'Choose a model' })).toBeNull();
  });

  it('keeps a failed Apple availability action open and puts its technical error in Details', async () => {
    render(<FailureChooser provider="apple" />);
    await userEvent.click(screen.getByRole('button', { name: 'Model' }));
    await userEvent.click(screen.getByRole('button', { name: 'Use Apple Model' }));

    expect(screen.getByRole('dialog', { name: 'Choose a model' })).toBeVisible();
    const error = screen.getByText(/Error: Foundation helper failed at \/private\/tmp\/apple\.log/);
    expect(error).not.toBeVisible();
    await userEvent.click(screen.getAllByText('Details')[0]!);
    expect(error).toBeVisible();
  });

  it('keeps a failed Qwen start open and puts its technical error in Details', async () => {
    render(<FailureChooser provider="qwen" />);
    await userEvent.click(screen.getByRole('button', { name: 'Model' }));
    await userEvent.click(screen.getByRole('button', { name: 'Use Qwen' }));

    expect(screen.getByRole('dialog', { name: 'Choose a model' })).toBeVisible();
    const error = screen.getByText(/Error: Managed server failed at \/private\/tmp\/qwen\.log/);
    expect(error).not.toBeVisible();
    await userEvent.click(screen.getAllByText('Details')[1]!);
    expect(error).toBeVisible();
  });

  it('closes after a successful selection advances its revision', async () => {
    render(<SuccessfulAppleChooser />);
    await userEvent.click(screen.getByRole('button', { name: 'Model' }));
    await userEvent.click(screen.getByRole('button', { name: 'Use Apple Model' }));

    expect(screen.queryByRole('dialog', { name: 'Choose a model' })).toBeNull();
  });
});

function ControlledChooser() {
  const [open, setOpen] = useState(false);
  const apple = entry({});
  const qwen = entry({
    id: 'qwen-coder-1.5b-mlx-4bit',
    displayName: 'Qwen Coder 1.5B',
    subtitle: 'Recommended for coding',
    providerId: 'mlx-lm',
    modelId: 'qwen-coder-1.5b-mlx-4bit',
    state: 'absent',
    downloadBytes: 868_628_559,
  });
  const catalog: ModelCatalogApi = {
    entries: [apple, qwen],
    entry: (id) => [apple, qwen].find((candidate) => candidate.id === id) ?? null,
    loading: false,
    downloadEventsReady: true,
    error: null,
    download: vi.fn().mockResolvedValue(undefined),
    cancelDownload: vi.fn().mockResolvedValue(undefined),
    useApple: vi.fn().mockResolvedValue(undefined),
    useQwen: vi.fn().mockResolvedValue(undefined),
    removeQwen: vi.fn().mockResolvedValue(undefined),
    refresh: vi.fn().mockResolvedValue(undefined),
  };
  const selection: SelectedModelApi = {
    selected: null,
    select: vi.fn(),
    clear: vi.fn(),
    revision: () => 0,
  };
  return (
    <>
      <ModelChooser open={open} onOpenChange={setOpen} catalog={catalog} selection={selection} />
      <button type="button">Outside</button>
    </>
  );
}

function DeferredAvailabilityChooser({ transition }: { transition: Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [appleAvailable, setAppleAvailable] = useState(true);
  useEffect(() => {
    void transition.then(() => setAppleAvailable(false));
  }, [transition]);
  const apple = entry(appleAvailable ? {} : {
    state: 'unavailable',
    availabilityReason: 'Apple Intelligence is turned off.',
  });
  const qwen = entry({
    id: 'qwen-coder-1.5b-mlx-4bit',
    displayName: 'Qwen Coder 1.5B',
    subtitle: 'Recommended for coding',
    providerId: 'mlx-lm',
    modelId: 'qwen-coder-1.5b-mlx-4bit',
    state: 'absent',
    downloadBytes: 868_628_559,
  });
  return (
    <>
      <ModelChooser
        open={open}
        onOpenChange={setOpen}
        catalog={catalogFor(apple, qwen)}
        selection={emptySelection()}
      />
      <button type="button">Outside</button>
    </>
  );
}

function FailureChooser({ provider }: { provider: 'apple' | 'qwen' }) {
  const [open, setOpen] = useState(false);
  const [failed, setFailed] = useState(false);
  const apple = entry({ error: failed ? 'Foundation helper failed at /private/tmp/apple.log' : null });
  const qwen = entry({
    id: 'qwen-coder-1.5b-mlx-4bit',
    displayName: 'Qwen Coder 1.5B',
    subtitle: 'Recommended for coding',
    providerId: 'mlx-lm',
    modelId: 'qwen-coder-1.5b-mlx-4bit',
    state: 'installed',
    downloadBytes: 868_628_559,
    error: failed ? 'Managed server failed at /private/tmp/qwen.log' : null,
  });
  const catalog = catalogFor(apple, qwen, {
    ...(provider === 'apple' ? { useApple: async () => setFailed(true) } : {}),
    ...(provider === 'qwen' ? { useQwen: async () => setFailed(true) } : {}),
  });
  return <ModelChooser open={open} onOpenChange={setOpen} catalog={catalog} selection={emptySelection()} />;
}

function SuccessfulAppleChooser() {
  const [open, setOpen] = useState(false);
  const revisionRef = useRef(0);
  const [selected, setSelected] = useState<SelectedModelApi['selected']>(null);
  const apple = entry({});
  const qwen = entry({
    id: 'qwen-coder-1.5b-mlx-4bit', displayName: 'Qwen Coder 1.5B',
    subtitle: 'Recommended for coding', providerId: 'mlx-lm',
    modelId: 'qwen-coder-1.5b-mlx-4bit', state: 'absent', downloadBytes: 868_628_559,
  });
  const selection: SelectedModelApi = {
    selected,
    select: (next) => {
      revisionRef.current += 1;
      setSelected(next);
    },
    clear: () => setSelected(null),
    revision: () => revisionRef.current,
  };
  const catalog = catalogFor(apple, qwen, {
    useApple: async () => selection.select({
      providerId: 'apple-foundation', providerDisplayName: 'Apple On-Device', modelId: 'system',
    }),
  });
  return <ModelChooser open={open} onOpenChange={setOpen} catalog={catalog} selection={selection} />;
}

function emptySelection(): SelectedModelApi {
  return { selected: null, select: vi.fn(), clear: vi.fn(), revision: () => 0 };
}

function catalogFor(
  apple: ModelCatalogEntry,
  qwen: ModelCatalogEntry,
  overrides: Partial<ModelCatalogApi> = {},
): ModelCatalogApi {
  return {
    entries: [apple, qwen],
    entry: (id) => [apple, qwen].find((candidate) => candidate.id === id) ?? null,
    loading: false,
    downloadEventsReady: true,
    error: null,
    download: vi.fn().mockResolvedValue(undefined),
    cancelDownload: vi.fn().mockResolvedValue(undefined),
    useApple: vi.fn().mockResolvedValue(undefined),
    useQwen: vi.fn().mockResolvedValue(undefined),
    removeQwen: vi.fn().mockResolvedValue(undefined),
    refresh: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}
