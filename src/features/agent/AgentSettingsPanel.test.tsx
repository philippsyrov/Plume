import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentSettingsPanel } from './AgentSettingsPanel';
import type { AgentConfig } from '../../lib/api/session';

// Mock the session IPC surface. The panel imports these as plain
// functions, so a hoisted module mock is enough — no Tauri bridge.
const mocks = vi.hoisted(() => ({
  getSessionState: vi.fn(),
  setAgentMode: vi.fn(),
  setApprovalPolicy: vi.fn(),
  setAllowlist: vi.fn(),
}));

vi.mock('../../lib/api/session', async (importOriginal) => {
  // Keep the real constants (AGENT_MODES, labels, caps) — only the IPC
  // functions are mocked.
  const real = await importOriginal<typeof import('../../lib/api/session')>();
  return {
    ...real,
    getSessionState: mocks.getSessionState,
    setAgentMode: mocks.setAgentMode,
    setApprovalPolicy: mocks.setApprovalPolicy,
    setAllowlist: mocks.setAllowlist,
  };
});

function defaultConfig(): AgentConfig {
  return {
    mode: 'chat',
    approvalPolicy: 'ask-each',
    fileAllowlist: [],
    commandAllowlist: [],
    iterationCap: null,
  };
}

describe('AgentSettingsPanel — D84', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getSessionState.mockResolvedValue(defaultConfig());
  });

  it('loads the current config and reflects it in the controls', async () => {
    mocks.getSessionState.mockResolvedValue({
      mode: 'scoped-edit',
      approvalPolicy: 'ask-on-write',
      fileAllowlist: ['src/'],
      commandAllowlist: [['cargo', 'test']],
      iterationCap: 8,
    });
    render(<AgentSettingsPanel />);

    const mode = (await screen.findByLabelText('Mode')) as HTMLSelectElement;
    expect(mode.value).toBe('scoped-edit');
    expect((screen.getByLabelText('Approval') as HTMLSelectElement).value).toBe(
      'ask-on-write',
    );
    expect(
      (screen.getByLabelText(/File allowlist/) as HTMLTextAreaElement).value,
    ).toBe('src/');
    expect(
      (screen.getByLabelText(/Command allowlist/) as HTMLTextAreaElement).value,
    ).toBe('cargo test');
    expect((screen.getByLabelText(/Iteration cap/) as HTMLInputElement).value).toBe('8');
  });

  it('flips mode immediately via session.setMode and adopts the returned state', async () => {
    mocks.setAgentMode.mockResolvedValue({
      ok: true,
      state: { ...defaultConfig(), mode: 'propose-diff' },
    });
    render(<AgentSettingsPanel />);

    const mode = (await screen.findByLabelText('Mode')) as HTMLSelectElement;
    await userEvent.selectOptions(mode, 'propose-diff');

    expect(mocks.setAgentMode).toHaveBeenCalledWith('propose-diff');
    await waitFor(() => expect(mode.value).toBe('propose-diff'));
  });

  it('surfaces backend validation reasons and leaves the mode unchanged on refusal', async () => {
    // The fail-closed rule: flipping to agent-loop without gates is
    // refused; the panel shows every reason and keeps mode at chat.
    mocks.setAgentMode.mockResolvedValue({
      ok: false,
      reasons: [
        'agent-loop requires a non-empty fileAllowlist',
        'agent-loop requires a non-empty commandAllowlist',
        'agent-loop requires an iterationCap',
      ],
    });
    render(<AgentSettingsPanel />);

    const mode = (await screen.findByLabelText('Mode')) as HTMLSelectElement;
    await userEvent.selectOptions(mode, 'agent-loop');

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('agent-loop requires a non-empty fileAllowlist');
    expect(alert).toHaveTextContent('agent-loop requires an iterationCap');
    // Mode select reverts to the committed value (still chat).
    expect(mode.value).toBe('chat');
  });

  it('parses the allowlist textareas into argv vectors on Apply', async () => {
    mocks.setAllowlist.mockResolvedValue({
      ok: true,
      state: {
        mode: 'chat',
        approvalPolicy: 'ask-each',
        fileAllowlist: ['src/', 'docs/'],
        commandAllowlist: [['cargo', 'test'], ['npm', 'run', 'build']],
        iterationCap: 5,
      },
    });
    render(<AgentSettingsPanel />);

    const files = (await screen.findByLabelText(/File allowlist/)) as HTMLTextAreaElement;
    const commands = screen.getByLabelText(/Command allowlist/) as HTMLTextAreaElement;
    const cap = screen.getByLabelText(/Iteration cap/) as HTMLInputElement;

    await userEvent.clear(files);
    await userEvent.type(files, 'src/\ndocs/');
    await userEvent.clear(commands);
    await userEvent.type(commands, 'cargo test\nnpm run build');
    await userEvent.clear(cap);
    await userEvent.type(cap, '5');
    await userEvent.click(screen.getByRole('button', { name: 'Apply gates' }));

    expect(mocks.setAllowlist).toHaveBeenCalledWith({
      fileAllowlist: ['src/', 'docs/'],
      commandAllowlist: [['cargo', 'test'], ['npm', 'run', 'build']],
      iterationCap: 5,
    });
  });

  it('treats a blank cap as no cap and blocks Apply on a non-numeric cap', async () => {
    render(<AgentSettingsPanel />);
    const cap = (await screen.findByLabelText(/Iteration cap/)) as HTMLInputElement;

    // Non-numeric: Apply disabled, inline error shown.
    await userEvent.type(cap, 'abc');
    const apply = screen.getByRole('button', { name: 'Apply gates' });
    expect(apply).toBeDisabled();
    expect(screen.getByText('cap must be a number')).toBeInTheDocument();

    // Blank: Apply enabled, sends iterationCap null.
    mocks.setAllowlist.mockResolvedValue({ ok: true, state: defaultConfig() });
    await userEvent.clear(cap);
    expect(apply).not.toBeDisabled();
    await userEvent.click(apply);
    expect(mocks.setAllowlist).toHaveBeenCalledWith({
      fileAllowlist: [],
      commandAllowlist: [],
      iterationCap: null,
    });
  });

  it('mirrors the committed mode upward on load and on change (D96)', async () => {
    mocks.getSessionState.mockResolvedValue({ ...defaultConfig(), mode: 'propose-diff' });
    mocks.setAgentMode.mockResolvedValue({
      ok: true,
      state: { ...defaultConfig(), mode: 'scoped-edit' },
    });
    const onModeChange = vi.fn();
    render(<AgentSettingsPanel onModeChange={onModeChange} />);

    // Initial load surfaces the loaded mode.
    await waitFor(() => expect(onModeChange).toHaveBeenCalledWith('propose-diff'));

    // A committed change surfaces the new mode too.
    const mode = (await screen.findByLabelText('Mode')) as HTMLSelectElement;
    await userEvent.selectOptions(mode, 'scoped-edit');
    await waitFor(() => expect(onModeChange).toHaveBeenCalledWith('scoped-edit'));
  });
});
