import { useState, type ChangeEvent } from 'react';

import { AgentDryRunPanel } from '../agent/AgentDryRunPanel';
import { AgentSettingsPanel } from '../agent/AgentSettingsPanel';
import { AgentSingleStepPanel } from '../agent/AgentSingleStepPanel';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import { MemoryPanel } from '../memory/MemoryPanel';
import { SkillsPanel } from '../skills/SkillsPanel';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import { LocalModelsPanel } from '../providers/LocalModelsPanel';
import { ProvidersPanel } from '../providers/ProvidersPanel';
import type { ProviderInventory } from '../providers/useProviderInventory';
import {
  MLX_LM_PROVIDER_ID,
  type MlxServersApi,
  type MlxServerStatus,
} from '../providers/useMlxServers';
import type { AgentMode } from '../../lib/api/session';
import type { LocalModel } from '../../lib/api/providers';
import type { ProjectWorkspaceView } from './UnifiedSidebar';

export function topbarSubtitle(
  activeView: ProjectWorkspaceView,
  projectName: string | null,
): string {
  if (activeView === 'files') return 'Files';
  if (activeView === 'benchmarks') return 'Benchmarks';
  if (activeView === 'knowledge') return 'Knowledge';
  if (activeView === 'browser') return 'Browser';
  if (activeView === 'local-chat') return 'Simple chat';
  return projectName ?? 'Project chat';
}

export function UnifiedTopBar({
  subtitle,
  inventory,
  servers,
  selected,
  onSelect,
  toolsOpen,
  showTools,
  showOpenProject,
  onToggleTools,
  onOpenProject,
}: {
  subtitle: string;
  inventory: ProviderInventory;
  servers: MlxServersApi;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
  toolsOpen: boolean;
  showTools: boolean;
  showOpenProject: boolean;
  onToggleTools: () => void;
  onOpenProject: () => void;
}) {
  return (
    <header className="plume-unified-topbar">
      <div className="plume-unified-brand">
        <h2 className="plume-unified-title">Plume</h2>
        <span className="plume-unified-subtitle">{subtitle}</span>
      </div>
      <div className="plume-unified-actions">
        <NoProjectModelPicker
          inventory={inventory}
          servers={servers}
          selected={selected}
          onSelect={onSelect}
        />
        {showOpenProject ? (
          <button type="button" className="ink-button" onClick={onOpenProject}>
            Open a project
          </button>
        ) : null}
        {showTools ? (
          <button
            type="button"
            className={`ink-button plume-tool-drawer-button${
              toolsOpen ? ' plume-tool-drawer-button-active' : ''
            }`}
            onClick={onToggleTools}
            aria-label={toolsOpen ? 'Close workspace views' : 'Open workspace views'}
            aria-pressed={toolsOpen}
            title="Workspace views"
          >
            <span className="plume-tool-drawer-button-icon" aria-hidden="true" />
            <span className="plume-visually-hidden">Workspace views</span>
          </button>
        ) : null}
      </div>
    </header>
  );
}

export function OpenProjectModal({
  onOpen,
  onClose,
}: {
  onOpen: (path: string) => void;
  onClose: () => void;
}) {
  const [path, setPath] = useState('');
  const trimmed = path.trim();
  const canOpen = trimmed.length > 0;
  return (
    <div
      className="plume-project-settings-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="plume-project-settings-window plume-open-project-window"
        role="dialog"
        aria-modal="true"
        aria-labelledby="plume-open-project-title"
      >
        <header className="plume-project-settings-header">
          <div>
            <h3 id="plume-open-project-title">Open a project</h3>
            <p>Paste a local folder path to add project context to this window.</p>
          </div>
          <button
            type="button"
            className="ink-button plume-project-settings-close"
            onClick={onClose}
            aria-label="Close open project"
          >
            Close
          </button>
        </header>
        <form
          className="plume-open-project-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (!canOpen) return;
            onOpen(trimmed);
            onClose();
          }}
        >
          <label className="plume-open-form-label">
            Project path
            <input
              type="text"
              className="plume-open-form-input"
              value={path}
              placeholder="/Users/you/code/some-project"
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              onChange={(event) => setPath(event.target.value)}
            />
          </label>
          <button type="submit" className="ink-button" disabled={!canOpen}>
            Open
          </button>
        </form>
      </section>
    </div>
  );
}

export function ProjectSettingsModal({
  inventory,
  servers,
  selected,
  onSelect,
  agentMode,
  onAgentModeChange,
  inspectorSelection,
  inspectorLineRange,
  onClose,
}: {
  inventory: ProviderInventory;
  servers: MlxServersApi;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
  agentMode: AgentMode | null;
  onAgentModeChange: (mode: AgentMode | null) => void;
  inspectorSelection: SelectionState | null;
  inspectorLineRange: EditorLineRange | null;
  onClose: () => void;
}) {
  return (
    <div
      className="plume-project-settings-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="plume-project-settings-window"
        role="dialog"
        aria-modal="true"
        aria-labelledby="plume-project-settings-title"
      >
        <header className="plume-project-settings-header">
          <div>
            <h3 id="plume-project-settings-title">Settings</h3>
            <p>Agent controls, providers, local models, project memory, and skills.</p>
          </div>
          <button
            type="button"
            className="ink-button plume-project-settings-close"
            onClick={onClose}
            aria-label="Close settings"
          >
            Close
          </button>
        </header>
        <div className="plume-project-settings-body">
          <AgentSettingsPanel onModeChange={onAgentModeChange} />
          <AgentSingleStepPanel
            selected={selected}
            mlxServers={servers}
            agentMode={agentMode}
            inspectorSelection={inspectorSelection}
            inspectorLineRange={inspectorLineRange}
          />
          <AgentDryRunPanel />
          <ProvidersPanel inventory={inventory} selected={selected} onSelect={onSelect} />
          <LocalModelsPanel
            inventory={inventory}
            servers={servers}
            selected={selected}
            onSelect={onSelect}
          />
          <MemoryPanel />
          <SkillsPanel />
        </div>
      </section>
    </div>
  );
}

export function NoProjectSettingsModal({
  inventory,
  servers,
  selected,
  onSelect,
  onClose,
}: {
  inventory: ProviderInventory;
  servers: MlxServersApi;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
  onClose: () => void;
}) {
  return (
    <div
      className="plume-project-settings-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="plume-project-settings-window"
        role="dialog"
        aria-modal="true"
        aria-labelledby="plume-no-project-settings-title"
      >
        <header className="plume-project-settings-header">
          <div>
            <h3 id="plume-no-project-settings-title">Settings</h3>
            <p>Providers and local model runtime controls.</p>
          </div>
          <button
            type="button"
            className="ink-button plume-project-settings-close"
            onClick={onClose}
            aria-label="Close settings"
          >
            Close
          </button>
        </header>
        <div className="plume-project-settings-body">
          <ProvidersPanel inventory={inventory} selected={selected} onSelect={onSelect} />
          <LocalModelsPanel
            inventory={inventory}
            servers={servers}
            selected={selected}
            onSelect={onSelect}
            noProject
          />
        </div>
      </section>
    </div>
  );
}

function NoProjectModelPicker({
  inventory,
  servers,
  selected,
  onSelect,
}: {
  inventory: ProviderInventory;
  servers: MlxServersApi;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
}) {
  const { state } = inventory;
  if (state.kind === 'loading') {
    return (
      <div className="plume-no-project-model-picker" role="status">
        <span className="plume-no-project-model-label">Model</span>
        <span className="plume-no-project-model-status">Loading local models…</span>
      </div>
    );
  }
  if (state.kind === 'error') {
    return (
      <div className="plume-no-project-model-picker" role="alert">
        <span className="plume-no-project-model-label">Model</span>
        <span className="plume-no-project-model-status">{state.message}</span>
      </div>
    );
  }

  const selectableModels = state.localModels.filter((model) =>
    canChatWithLocalModel(model, servers.statusOf(model.id)),
  );
  const selectedIsLocal =
    selected?.providerId === MLX_LM_PROVIDER_ID &&
    selectableModels.some((model) => model.id === selected.modelId);
  const value = selectedIsLocal ? selected.modelId : '';

  const onChange = (event: ChangeEvent<HTMLSelectElement>) => {
    const model = selectableModels.find((candidate) => candidate.id === event.target.value);
    if (!model) return;
    selectNoProjectLocalModel(model, onSelect);
  };

  return (
    <div className="plume-no-project-model-picker">
      <label className="plume-no-project-model-label" htmlFor="plume-no-project-model">
        Model
      </label>
      <select
        id="plume-no-project-model"
        className="plume-no-project-model-select"
        value={value}
        onChange={onChange}
        disabled={selectableModels.length === 0}
      >
        <option value="">
          {selectableModels.length === 0 ? 'No running local model' : 'Pick a model'}
        </option>
        {selectableModels.map((model) => (
          <option key={model.id} value={model.id}>
            {model.name}
          </option>
        ))}
      </select>
      <span className="plume-no-project-model-status">
        {modelPickerStatusText(state.localModels, selectableModels, selected, servers)}
      </span>
    </div>
  );
}

function canChatWithLocalModel(model: LocalModel, status: MlxServerStatus): boolean {
  if (model.kind !== 'mlx-folder' && model.kind !== 'transformer-folder') return false;
  return status.kind === 'running';
}

function selectNoProjectLocalModel(
  model: LocalModel,
  onSelect: (next: SelectedModel) => void,
): void {
  onSelect({
    providerId: MLX_LM_PROVIDER_ID,
    providerDisplayName: 'MLX (Plume-managed)',
    modelId: model.id,
  });
}

function modelPickerStatusText(
  models: LocalModel[],
  selectableModels: LocalModel[],
  selected: SelectedModel | null,
  servers: MlxServersApi,
): string {
  if (models.length === 0) return 'No local model folders found.';
  if (selectableModels.length === 0) {
    return 'Start a local model from a trusted project, then pick it here.';
  }
  if (selected?.providerId !== MLX_LM_PROVIDER_ID) {
    return 'Choose a running local model for chat.';
  }
  const status = servers.statusOf(selected.modelId);
  if (status.kind === 'running') return `Running on port ${status.handle.port}.`;
  return 'Choose a running local model for chat.';
}
