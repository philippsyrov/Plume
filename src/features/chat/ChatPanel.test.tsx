import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import '../../styles/layout/chat.css';
import type { ChatApi } from './useChat';
import type { ChatContextPreviewState } from './useChatContextPreview';
import { ChatPanel } from './ChatPanel';
import type { ProviderReachabilityState } from './useProviderReachability';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServersApi } from '../providers/useMlxServers';
import type { ServerHandle } from '../../lib/api/providers';

const chatCss = readFileSync(
  join(process.cwd(), 'src/styles/layout/chat.css'),
  'utf8',
);

const mocks = vi.hoisted(() => ({
  useChat: vi.fn(),
  useChatContextPreview: vi.fn(),
  useProviderReachability: vi.fn(),
}));

vi.mock('./useChat', () => ({ useChat: mocks.useChat }));
vi.mock('./useChatContextPreview', () => ({
  useChatContextPreview: mocks.useChatContextPreview,
}));
vi.mock('./useProviderReachability', () => ({
  useProviderReachability: mocks.useProviderReachability,
}));

const qwenSelection: SelectedModel = {
  providerId: 'mlx-lm',
  providerDisplayName: 'MLX (Plume-managed)',
  modelId: 'plume-model-dir:Qwen2.5-Coder-3B-Instruct-4bit',
};

const runningHandle: ServerHandle = {
  id: 'mlx-handle-test',
  port: 64606,
  pid: 12345,
};

describe('ChatPanel', () => {
  beforeEach(() => {
    mocks.useChat.mockReturnValue(makeChatApi());
    mocks.useChatContextPreview.mockReturnValue(makeContextPreview());
    mocks.useProviderReachability.mockReturnValue(makeReachability());
  });

  it('shows the topics badge when the last send folded in topic files', () => {
    mocks.useChat.mockReturnValue({
      ...makeChatApi(),
      lastTopicsUsed: { fileCount: 2, bytes: 120, byteCap: 6144, truncated: false },
    });

    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
      />,
    );

    expect(screen.getByText(/Topics · 2 files · 120 B/)).toBeInTheDocument();
  });

  it('hides the topics badge when no topic files are involved', () => {
    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
      />,
    );

    expect(screen.queryByText(/Topics ·/)).not.toBeInTheDocument();
  });

  it('renders a disabled textarea when no model is selected', () => {
    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
      />,
    );

    const textarea = screen.getByLabelText('Message to send');
    expect(textarea).toBeVisible();
    expect(textarea).toBeDisabled();
    expect(textarea).toHaveAttribute(
      'placeholder',
      'Pick a running model to enable chat.',
    );
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled();
  });

  it('keeps the textarea visible but gates Send for an MLX model without a handle', async () => {
    render(
      <ChatPanel
        selected={qwenSelection}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
      />,
    );

    const textarea = screen.getByLabelText('Message to send');
    expect(textarea).toBeVisible();
    expect(textarea).not.toBeDisabled();
    expect(textarea).toHaveAttribute(
      'placeholder',
      `Type your message — start ${qwenSelection.modelId} from Settings to send.`,
    );

    await userEvent.type(textarea, 'say hi');

    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled();
  });

  it('keeps the form non-shrinking and enables Send for a running MLX handle', async () => {
    render(
      <ChatPanel
        selected={qwenSelection}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(runningHandle)}
      />,
    );

    const form = document.querySelector<HTMLFormElement>('.plume-chat-form');
    const textarea = screen.getByLabelText('Message to send');
    const sendButton = screen.getByRole('button', { name: 'Send message' });

    expect(form).not.toBeNull();
    expect(form).toBeVisible();
    expect(textarea).toBeVisible();
    expect(textarea).not.toBeDisabled();
    expect(sendButton).toBeDisabled();
    expect(screen.getByText(qwenSelection.modelId)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Stop' })).toBeVisible();

    expect(chatCss).toMatch(
      /\.plume-chat-form\s*\{[^}]*flex:\s*0 0 auto[^}]*\}/s,
    );

    await userEvent.type(textarea, 'say hi');

    expect(sendButton).toBeEnabled();
  });

  it('simple variant exposes no attach or project-context affordances (D63B)', () => {
    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
        includeProjectContext={false}
        variant="simple"
      />,
    );

    expect(screen.queryByRole('button', { name: /Attach/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/Project instructions/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Memory ·/)).not.toBeInTheDocument();
    expect(
      screen.getByText(/Local chat\. No project context is included\./),
    ).toBeInTheDocument();
  });

  it('identifies project context in an empty project chat', () => {
    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
        includeProjectContext
        variant="simple"
      />,
    );

    expect(
      screen.getByText(/Project chat\. Project context is enabled for messages\./),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Project context is included\./)).not.toBeInTheDocument();
  });

  it('renders an externally-owned chat instance when the session shell passes one (D63B)', () => {
    const external: ChatApi = {
      ...makeChatApi(),
      entries: [
        { kind: 'message', message: { role: 'user', content: 'restored question' } },
        { kind: 'message', message: { role: 'assistant', content: 'restored answer' } },
      ],
    };
    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
        variant="simple"
        chat={external}
      />,
    );

    expect(screen.getByText('restored question')).toBeInTheDocument();
    expect(screen.getByText('restored answer')).toBeInTheDocument();
  });
});

function makeChatApi(): ChatApi {
  return {
    entries: [],
    status: 'idle',
    lastError: null,
    activeStreamId: null,
    lastInstructionsIncluded: null,
    lastMemoryUsed: null,
    lastTopicsUsed: null,
    send: vi.fn().mockResolvedValue('accepted'),
    cancel: vi.fn().mockResolvedValue(undefined),
    clear: vi.fn(),
    restore: vi.fn(),
  };
}

function makeContextPreview(): ChatContextPreviewState {
  return {
    status: 'ready',
    data: { instructions: null, attachment: null, memory: null, topics: null },
    error: null,
  };
}

function makeReachability(): ProviderReachabilityState {
  return {
    status: 'ready',
    reachability: 'available',
    latencyMs: 1,
    error: null,
    refresh: vi.fn(),
  };
}

function makeMlxServers(handle: ServerHandle | null): MlxServersApi {
  return {
    statuses: new Map(),
    statusOf: () => (handle ? { kind: 'running', handle } : { kind: 'idle' }),
    handleOf: () => handle,
    start: vi.fn().mockResolvedValue(handle),
    stop: vi.fn().mockResolvedValue(undefined),
    clearError: vi.fn(),
  };
}
