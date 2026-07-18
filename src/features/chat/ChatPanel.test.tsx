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
const projectShellCss = readFileSync(
  join(process.cwd(), 'src/styles/layout/project-shell.css'),
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

const appleSelection: SelectedModel = {
  providerId: 'apple-foundation',
  providerDisplayName: 'Apple On-Device',
  modelId: 'system',
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
      lastTopicsUsed: {
        fileCount: 2,
        bytes: 120,
        byteCap: 6144,
        truncated: false,
        files: [
          { name: 'INDEX.md', bytes: 70 },
          { name: 'USER.md', bytes: 50 },
        ],
      },
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

    expect(screen.getByText(/Topics · 2 files/)).toBeInTheDocument();
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

  it('keeps context chips contained at constrained widths', () => {
    expect(chatCss).toContain('.plume-chat-title {');
    expect(chatCss).toMatch(/\.plume-chat-title\s*\{[^}]*flex-wrap:\s*wrap/s);
    expect(chatCss).toMatch(/\.plume-chat-title\s*\{[^}]*min-width:\s*0/s);
    expect(chatCss).toMatch(
      /\.plume-chat-context-manifest:last-of-type > \.plume-disclosure-content\s*\{[^}]*right:\s*0/s,
    );
  });

  it('renders a disabled textarea and chooser action when no model is selected', async () => {
    const onChooseModel = vi.fn();
    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        onChooseModel={onChooseModel}
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
      'Choose a model to start chatting.',
    );
    const choose = screen.getByRole('button', { name: 'Choose a model' });
    expect(choose).toBeVisible();
    await userEvent.click(choose);
    expect(onChooseModel).toHaveBeenCalledOnce();
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

  it('sends Apple on-device chat without an Ollama probe or MLX handle', async () => {
    const chat = makeChatApi();
    render(
      <ChatPanel
        selected={appleSelection}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
        includeProjectContext={false}
        variant="simple"
        chat={chat}
      />,
    );

    await userEvent.type(screen.getByLabelText('Message to send'), 'Hello from Apple');
    await userEvent.click(screen.getByRole('button', { name: 'Send message' }));

    expect(chat.send).toHaveBeenCalledWith(
      'apple-foundation',
      'system',
      'Hello from Apple',
      { includeProjectContext: false },
    );
  });

  it('keeps the effective project chrome focus indicator on the simple composer', () => {
    expect(projectShellCss).toMatch(
      /\.plume-project-codex :is\(button, select, textarea, input\):focus-visible\s*\{[^}]*outline:\s*1px solid var\(--plume-chrome-line-strong\)[^}]*outline-offset:\s*2px[^}]*\}/s,
    );
    expect(chatCss).not.toMatch(
      /\.plume-chat-simple \.plume-chat-input:focus-visible\s*\{[^}]*outline:\s*none[^}]*\}/s,
    );
  });

  it('opens with a familiar empty composer instead of implementation copy', () => {
    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
        variant="simple"
      />,
    );

    expect(screen.getByText('What can I help you with?')).toBeInTheDocument();
    expect(screen.queryByText(/streaming read-only chat/i)).not.toBeInTheDocument();
  });

  it('keeps instructions neutral while loading, then shows an honest ready skip', () => {
    mocks.useChatContextPreview.mockReturnValue({
      status: 'loading',
      data: null,
      error: null,
    });
    const props = {
      selected: null,
      onClearSelection: vi.fn(),
      inspectorSelection: null,
      inspectorLineRange: null,
      projectHasInstructions: true,
      mlxServers: makeMlxServers(null),
      variant: 'simple' as const,
    };
    const { rerender } = render(<ChatPanel {...props} />);

    expect(screen.getByRole('status', { name: 'Checking project instructions.' })).toBeVisible();
    expect(document.querySelector('.plume-chat-instructions-badge-skipped')).toBeNull();

    mocks.useChatContextPreview.mockReturnValue({
      status: 'ready',
      data: {
        instructions: null,
        attachment: null,
        memory: null,
        topics: null,
        contextSources: [],
      },
      error: null,
    });
    rerender(<ChatPanel {...props} />);

    expect(
      screen.getByRole('status', { name: 'Project instructions are unavailable for the next send.' }),
    ).toBeVisible();
    expect(document.querySelector('.plume-chat-instructions-badge-skipped')).not.toBeNull();
  });

  it('keeps normal chat implicit and exposes file changes as a quiet secondary action', async () => {
    const chat = makeChatApi();
    render(
      <ChatPanel
        selected={qwenSelection}
        onClearSelection={vi.fn()}
        inspectorSelection={null}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(runningHandle)}
        variant="simple"
        chat={chat}
      />,
    );

    expect(screen.queryByLabelText('Action for this message')).not.toBeInTheDocument();
    const makeChanges = screen.getByRole('button', { name: 'Make changes' });
    expect(makeChanges).toHaveAttribute('aria-pressed', 'false');
    expect(screen.queryByText(/Draft a code change/)).not.toBeInTheDocument();

    await userEvent.click(makeChanges);
    expect(screen.getByRole('button', { name: 'Make changes' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(screen.getByRole('button', { name: 'Make changes' })).toHaveTextContent(
      'Making changes',
    );
    expect(
      screen.getByText('Plume will draft a file change. You still choose whether to apply it.'),
    ).toBeInTheDocument();

    await userEvent.type(screen.getByLabelText('Message to send'), 'Rename this helper');
    await userEvent.click(screen.getByRole('button', { name: 'Send message' }));

    expect(chat.send).toHaveBeenCalledWith(
      qwenSelection.providerId,
      qwenSelection.modelId,
      'Rename this helper',
      expect.objectContaining({ mode: 'proposeDiff' }),
    );
  });

  it('project simple exposes badges, selection attachment, and context preview', async () => {
    mocks.useChatContextPreview.mockImplementation((input: { relPath: string | null }) => ({
      status: 'ready',
      data: {
        instructions: {
          source: 'AGENTS.md',
          originalBytes: 420,
          redactionCount: 0,
        },
        attachment:
          input.relPath === null
            ? null
            : {
                status: 'ready',
                relPath: input.relPath,
                originalBytes: 96,
                redactionCount: 1,
                startLine: 12,
                endLine: 18,
              },
        memory: {
          entryCount: 1,
          bytes: 24,
          byteCap: 4096,
          truncated: false,
          entries: [
            {
              id: 'm_0123456789abcdef0123456789abcdef',
              createdAtMs: 1_700_000_000_000,
              textBytes: 24,
              preview: 'Prefer concise answers.',
            },
          ],
        },
        topics: {
          fileCount: 2,
          bytes: 180,
          byteCap: 6144,
          truncated: false,
          files: [
            { name: 'INDEX.md', bytes: 100 },
            { name: 'USER.md', bytes: 80 },
          ],
        },
      },
      error: null,
    }));

    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={{
          kind: 'ready',
          path: 'src/App.tsx',
          content: { content: 'line 12\nline 13', encoding: 'utf-8', bytes: 640 },
        }}
        inspectorLineRange={{ startLine: 12, endLine: 18 }}
        projectHasInstructions
        mlxServers={makeMlxServers(null)}
        variant="simple"
      />,
    );

    expect(screen.getByText('Project instructions')).toBeInTheDocument();
    expect(screen.getByText('Memory · 1 entry')).toBeInTheDocument();
    expect(screen.getByText('Topics · 2 files')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Use selection in chat' })).toBeInTheDocument();
    expect(
      screen.getByText('Inspector has lines 12–18 of src/App.tsx selected.'),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Use selection in chat' }));
    expect(mocks.useChat.mock.results.at(-1)?.value.addContextSource).toHaveBeenCalledWith({
      kind: 'projectFile',
      relPath: 'src/App.tsx',
      startLine: 12,
      endLine: 18,
    });
  });

  it('local simple hides every project-context affordance (D63B)', () => {
    mocks.useChatContextPreview.mockReturnValue({
      status: 'ready',
      data: {
        instructions: {
          source: 'AGENTS.md',
          originalBytes: 420,
          redactionCount: 0,
        },
        attachment: null,
        memory: {
          entryCount: 1,
          bytes: 24,
          byteCap: 4096,
          truncated: false,
          entries: [
            {
              id: 'm_0123456789abcdef0123456789abcdef',
              createdAtMs: 1_700_000_000_000,
              textBytes: 24,
              preview: 'Prefer concise answers.',
            },
          ],
        },
        topics: {
          fileCount: 2,
          bytes: 180,
          byteCap: 6144,
          truncated: false,
          files: [
            { name: 'INDEX.md', bytes: 100 },
            { name: 'USER.md', bytes: 80 },
          ],
        },
      },
      error: null,
    });

    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={{
          kind: 'ready',
          path: 'src/App.tsx',
          content: { content: 'line 12\nline 13', encoding: 'utf-8', bytes: 640 },
        }}
        inspectorLineRange={{ startLine: 12, endLine: 18 }}
        projectHasInstructions
        mlxServers={makeMlxServers(null)}
        includeProjectContext={false}
        variant="simple"
      />,
    );

    expect(screen.queryByRole('button', { name: /Attach/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/AGENTS\.md/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Memory ·/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Topics ·/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Context preview for next send')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Action for this message')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Make changes' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Use current file in chat' })).not.toBeInTheDocument();
    expect(screen.getByText(/Ask anything using the selected local model\./)).toBeInTheDocument();
  });

  it('drops a stale project action when the same composer becomes local chat', async () => {
    const chat = makeChatApi();
    const props = {
      selected: qwenSelection,
      onClearSelection: vi.fn(),
      inspectorSelection: null,
      inspectorLineRange: null,
      projectHasInstructions: false,
      mlxServers: makeMlxServers(runningHandle),
      variant: 'simple' as const,
      chat,
    };
    const { rerender } = render(<ChatPanel {...props} includeProjectContext />);

    await userEvent.click(screen.getByRole('button', { name: 'Make changes' }));
    rerender(<ChatPanel {...props} includeProjectContext={false} />);

    expect(screen.queryByLabelText('Action for this message')).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText('Message to send'), 'Explain this idea');
    await userEvent.click(screen.getByRole('button', { name: 'Send message' }));

    expect(chat.send).toHaveBeenCalledWith(
      qwenSelection.providerId,
      qwenSelection.modelId,
      'Explain this idea',
      { handleId: runningHandle.id, includeProjectContext: false },
    );
  });

  it('keeps explicit local context visible and removable without project context', async () => {
    const source = { kind: 'userMemoryEntry' as const, entryId: `m_${'a'.repeat(32)}` };
    const chat = { ...makeChatApi(), contextSources: [source] };
    mocks.useChatContextPreview.mockReturnValue({
      ...makeContextPreview(),
      data: {
        ...makeContextPreview().data,
        contextSources: [{
          status: 'ready',
          source: {
            ...source,
            createdAtMs: 1,
            bytes: 22,
            preview: 'Use worked examples.',
          },
        }],
      },
    });

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
        chat={chat}
      />,
    );

    expect(screen.getByText('Use worked examples.')).toBeVisible();
    await userEvent.click(
      screen.getByRole('button', { name: 'Remove Use worked examples. from context' }),
    );
    expect(chat.removeContextSource).toHaveBeenCalledWith(source);
  });

  it('hides project change controls when no model is running', () => {
    render(
      <ChatPanel
        selected={null}
        onClearSelection={vi.fn()}
        inspectorSelection={{ kind: 'empty' }}
        inspectorLineRange={null}
        projectHasInstructions={false}
        mlxServers={makeMlxServers(null)}
        includeProjectContext
        variant="simple"
      />,
    );

    expect(screen.queryByRole('button', { name: 'Make changes' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Use current file in chat' })).not.toBeInTheDocument();
    expect(chatCss).toMatch(
      /\.plume-chat-simple \.plume-chat-input:disabled\s*\{[^}]*min-height:\s*52px/s,
    );
  });

  it('keeps context source cards readable and contained at narrow widths', () => {
    expect(chatCss).toMatch(
      /\.plume-context-shelf-list\s*\{[^}]*display:\s*grid[^}]*grid-template-columns:\s*repeat\(auto-fit,/s,
    );
    expect(chatCss).toMatch(
      /\.plume-context-shelf-item\s*\{[^}]*min-width:\s*0[^}]*font-family:\s*var\(--font-ui\)/s,
    );
    expect(chatCss).toMatch(
      /\.plume-context-shelf-item\s*\{[^}]*grid-template-areas:\s*'kind remove'\s*'name remove'\s*'details remove'/s,
    );
    expect(chatCss).toMatch(
      /\.ink-badge\.plume-context-shelf-item\s*\{[^}]*font-family:\s*var\(--font-ui\)/s,
    );
    expect(chatCss).toMatch(
      /\.plume-context-shelf-name\s*\{[^}]*min-width:\s*0[^}]*-webkit-line-clamp:\s*2/s,
    );
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

    expect(screen.getByText('What can I help you with?')).toBeInTheDocument();
    expect(
      screen.getByText(
        /Project memory and topics may be included; sources you add are pinned exactly\./,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/only the context you choose/i)).not.toBeInTheDocument();
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
    contextSources: [],
    status: 'idle',
    lastError: null,
    activeStreamId: null,
    lastInstructionsIncluded: null,
    lastMemoryUsed: null,
    lastTopicsUsed: null,
    addContextSource: vi.fn(() => 'added' as const),
    removeContextSource: vi.fn(() => true),
    send: vi.fn().mockResolvedValue('accepted'),
    cancel: vi.fn().mockResolvedValue(undefined),
    clear: vi.fn(),
    restore: vi.fn(),
  };
}

function makeContextPreview(): ChatContextPreviewState {
  return {
    status: 'ready',
    data: {
      instructions: null,
      attachment: null,
      memory: null,
      topics: null,
      contextSources: [],
    },
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
    startCatalog: vi.fn().mockResolvedValue(handle),
    stop: vi.fn().mockResolvedValue(undefined),
    clearError: vi.fn(),
  };
}
