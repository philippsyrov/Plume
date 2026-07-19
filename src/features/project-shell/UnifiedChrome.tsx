import { useCallback, useState } from 'react';

import { AgentSettingsPanel } from '../agent/AgentSettingsPanel';
import { AgentSingleStepPanel } from '../agent/AgentSingleStepPanel';
import { AppearancePanel } from '../appearance/AppearancePanel';
import type { useAppearance } from '../appearance/useAppearance';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import { LibrarySettingsPanel } from '../library/LibrarySettingsPanel';
import { SkillsPanel } from '../skills/SkillsPanel';
import { ModelChooser } from '../model-picker/ModelChooser';
import type { ModelCatalogApi } from '../model-picker/useModelCatalog';
import type { SelectedModel, SelectedModelApi } from '../model-picker/useSelectedModel';
import { LocalModelsPanel } from '../providers/LocalModelsPanel';
import { ProvidersPanel } from '../providers/ProvidersPanel';
import type { ProviderInventory } from '../providers/useProviderInventory';
import type { MlxServersApi } from '../providers/useMlxServers';
import type { AgentMode } from '../../lib/api/session';
import { ModalDialog } from './ModalDialog';
import { SettingsCategoryLayout } from './SettingsCategoryLayout';
import type { ProjectWorkspaceView } from './UnifiedSidebar';

const SIDEBAR_PREFERENCE_KEY = 'plume:sidebar-v1';

export function readSidebarCollapsed(): boolean {
  try {
    const raw = localStorage.getItem(SIDEBAR_PREFERENCE_KEY);
    if (raw === null) return false;
    const parsed: unknown = JSON.parse(raw);
    return (
      typeof parsed === 'object' &&
      parsed !== null &&
      'collapsed' in parsed &&
      (parsed as { collapsed?: unknown }).collapsed === true
    );
  } catch {
    return false;
  }
}

export function writeSidebarCollapsed(collapsed: boolean): void {
  try {
    localStorage.setItem(SIDEBAR_PREFERENCE_KEY, JSON.stringify({ collapsed }));
  } catch {
    // The controlled React state remains authoritative for this window.
  }
}

export function useSidebarPreference(): readonly [boolean, (collapsed: boolean) => void] {
  const [collapsed, setCollapsed] = useState(readSidebarCollapsed);
  const update = useCallback((next: boolean) => {
    setCollapsed(next);
    writeSidebarCollapsed(next);
  }, []);
  return [collapsed, update] as const;
}

export function topbarSubtitle(
  activeView: ProjectWorkspaceView,
  projectName: string | null,
  activeSessionTitle: string | null = null,
): string {
  if (activeView === 'files') return 'Files';
  if (activeView === 'benchmarks') return 'Benchmarks';
  if (activeView === 'library') return 'Library';
  if (activeView === 'browser') return activeSessionTitle ?? 'Browser';
  if (activeView === 'local-chat') return activeSessionTitle ?? 'Chat';
  return activeSessionTitle ?? projectName ?? 'Project';
}

export function UnifiedTopBar({
  subtitle,
  catalog,
  selection,
  modelChooserOpen,
  onModelChooserOpenChange,
  toolsOpen,
  showTools,
  showOpenProject,
  onToggleTools,
  onOpenProject,
}: {
  subtitle: string;
  catalog: ModelCatalogApi;
  selection: SelectedModelApi;
  modelChooserOpen: boolean;
  onModelChooserOpenChange: (open: boolean) => void;
  toolsOpen: boolean;
  showTools: boolean;
  showOpenProject: boolean;
  onToggleTools: () => void;
  onOpenProject: () => void;
}) {
  return (
    <header className="plume-unified-topbar">
      <div className="plume-unified-brand">
        <h2 className="plume-unified-title plume-unified-subtitle">{subtitle}</h2>
      </div>
      <div
        className="plume-unified-drag-region"
        data-tauri-drag-region="true"
        aria-hidden="true"
      />
      <div className="plume-unified-actions" data-tauri-drag-region="false">
        <ModelChooser
          open={modelChooserOpen}
          onOpenChange={onModelChooserOpenChange}
          catalog={catalog}
          selection={selection}
        />
        {showOpenProject ? (
          <button
            type="button"
            className="ink-button"
            data-tauri-drag-region="false"
            onClick={onOpenProject}
          >
            Open a project
          </button>
        ) : null}
        {showTools ? (
          <button
            type="button"
            className={`ink-button plume-tool-drawer-button${
              toolsOpen ? ' plume-tool-drawer-button-active' : ''
            }`}
            data-tauri-drag-region="false"
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
    <ModalDialog
      labelledBy="plume-open-project-title"
      className="plume-open-project-window"
      onClose={onClose}
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
    </ModalDialog>
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
  appearance,
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
  appearance: ReturnType<typeof useAppearance>;
  onClose: () => void;
}) {
  return (
    <ModalDialog labelledBy="plume-project-settings-title" onClose={onClose}>
      <header className="plume-project-settings-header">
        <h3 id="plume-project-settings-title">Settings</h3>
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
        <SettingsCategoryLayout
          categories={[
            {
              id: 'general',
              label: 'General',
              content: (
                <AppearancePanel
                  value={appearance.preference}
                  onChange={appearance.setPreference}
                />
              ),
            },
            {
              id: 'models',
              label: 'Models',
              description: 'Choose what runs locally on this Mac.',
              content: (
                <div className="plume-settings-models">
                  <ProvidersPanel
                    inventory={inventory}
                    selected={selected}
                    onSelect={onSelect}
                  />
                  <LocalModelsPanel
                    inventory={inventory}
                    servers={servers}
                    selected={selected}
                    onSelect={onSelect}
                  />
                </div>
              ),
            },
            {
              id: 'personal',
              label: 'Personal',
              content: <LibrarySettingsPanel projectAvailable scope="personal" />,
            },
            {
              id: 'project',
              label: 'Project',
              content: <LibrarySettingsPanel projectAvailable scope="project" />,
            },
            {
              id: 'advanced',
              label: 'Advanced',
              content: (
                <div className="plume-project-settings-advanced-body">
                  <AgentSettingsPanel onModeChange={onAgentModeChange} />
                  <AgentSingleStepPanel
                    selected={selected}
                    mlxServers={servers}
                    agentMode={agentMode}
                    inspectorSelection={inspectorSelection}
                    inspectorLineRange={inspectorLineRange}
                  />
                  <SkillsPanel />
                </div>
              ),
            },
          ]}
        />
      </div>
    </ModalDialog>
  );
}

export function NoProjectSettingsModal({
  inventory,
  servers,
  selected,
  onSelect,
  appearance,
  onClose,
}: {
  inventory: ProviderInventory;
  servers: MlxServersApi;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
  appearance: ReturnType<typeof useAppearance>;
  onClose: () => void;
}) {
  return (
    <ModalDialog labelledBy="plume-no-project-settings-title" onClose={onClose}>
      <header className="plume-project-settings-header">
        <h3 id="plume-no-project-settings-title">Settings</h3>
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
        <SettingsCategoryLayout
          categories={[
            {
              id: 'general',
              label: 'General',
              content: (
                <AppearancePanel
                  value={appearance.preference}
                  onChange={appearance.setPreference}
                />
              ),
            },
            {
              id: 'models',
              label: 'Models',
              description: 'Choose what runs locally on this Mac.',
              content: (
                <div className="plume-settings-models">
                  <ProvidersPanel
                    inventory={inventory}
                    selected={selected}
                    onSelect={onSelect}
                  />
                  <LocalModelsPanel
                    inventory={inventory}
                    servers={servers}
                    selected={selected}
                    onSelect={onSelect}
                    noProject
                  />
                </div>
              ),
            },
            {
              id: 'personal',
              label: 'Personal',
              content: (
                <LibrarySettingsPanel projectAvailable={false} scope="personal" />
              ),
            },
            {
              id: 'project',
              label: 'Project',
              content: (
                <LibrarySettingsPanel projectAvailable={false} scope="project" />
              ),
            },
          ]}
        />
      </div>
    </ModalDialog>
  );
}
