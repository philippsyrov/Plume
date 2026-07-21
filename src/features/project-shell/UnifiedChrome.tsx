import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';

import { AgentSettingsPanel } from '../agent/AgentSettingsPanel';
import { AgentSingleStepPanel } from '../agent/AgentSingleStepPanel';
import { AppearancePanel } from '../appearance/AppearancePanel';
import type { useAppearance } from '../appearance/useAppearance';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import { LibrarySettingsPanel } from '../library/LibrarySettingsPanel';
import { SkillsPanel } from '../skills/SkillsPanel';
import { ModelChooserTrigger } from '../model-picker/ModelChooser';
import type { ModelCatalogApi } from '../model-picker/useModelCatalog';
import type { SelectedModel, SelectedModelApi } from '../model-picker/useSelectedModel';
import { LocalModelsPanel } from '../providers/LocalModelsPanel';
import { ProvidersPanel } from '../providers/ProvidersPanel';
import type { ProviderInventory } from '../providers/useProviderInventory';
import type { MlxServersApi } from '../providers/useMlxServers';
import type { AgentMode } from '../../lib/api/session';
import { chooseProjectFolder } from '../../lib/api/project';
import { ModalDialog } from './ModalDialog';
import { SettingsCategoryLayout } from './SettingsCategoryLayout';
import type { ProjectWorkspaceView } from './UnifiedSidebar';
import { useProjectFolderDrop } from './useProjectFolderDrop';

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
        <ModelChooserTrigger
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

export function OpenProjectView({
  onOpen,
  onClose,
  busy = false,
}: {
  onOpen: (path: string) => Promise<boolean>;
  onClose: () => void;
  busy?: boolean;
}) {
  const [path, setPath] = useState('');
  const [choosing, setChoosing] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const choosingRef = useRef(false);
  const submittingRef = useRef(false);
  const trimmed = path.trim();
  const unavailable = busy || choosing || submitting;
  const canOpen = trimmed.length > 0 && !unavailable;

  useEffect(() => () => {
    mountedRef.current = false;
  }, []);

  const submitCandidate = useCallback(async (candidate: string) => {
    if (busy || choosingRef.current || submittingRef.current) return;
    submittingRef.current = true;
    setError(null);
    setSubmitting(true);
    try {
      const opened = await onOpen(candidate);
      if (!mountedRef.current) return;
      if (opened) onClose();
      else setError('Couldn’t open this folder. Check the folder and try again.');
    } finally {
      submittingRef.current = false;
      if (mountedRef.current) setSubmitting(false);
    }
  }, [busy, onClose, onOpen]);

  useProjectFolderDrop({ busy: unavailable, onCandidate: submitCandidate });

  const chooseFolder = async () => {
    if (busy || choosingRef.current || submittingRef.current) return;
    choosingRef.current = true;
    setError(null);
    setChoosing(true);
    try {
      const candidate = await chooseProjectFolder();
      if (!mountedRef.current) return;
      choosingRef.current = false;
      setChoosing(false);
      if (candidate !== null) await submitCandidate(candidate);
    } catch {
      if (!mountedRef.current) return;
      choosingRef.current = false;
      setChoosing(false);
      setError('Couldn’t open the folder chooser. Enter a path instead.');
    }
  };

  return (
    <section
      className="plume-open-project-view"
      role="region"
      aria-labelledby="plume-open-project-title"
    >
      <header className="plume-inline-workspace-header">
        <div>
          <h3 id="plume-open-project-title">Open a project</h3>
          <p>Choose a folder to use its files and project tools.</p>
        </div>
        <button
          type="button"
          className="ink-button"
          onClick={onClose}
          aria-label="Back from open project"
        >
          Back
        </button>
      </header>
      <div className="plume-open-project-form">
        <button
          type="button"
          className="ink-button plume-open-project-choose"
          disabled={unavailable}
          onClick={() => void chooseFolder()}
        >
          {choosing ? 'Choosing…' : 'Choose folder…'}
        </button>
        <div className="plume-open-project-drop" aria-label="Folder drop area">
          <strong>Drop a folder from Finder</strong>
          <span>Plume will ask you to trust it before using project context.</span>
        </div>
        <div className="plume-open-project-manual">
          <button
            type="button"
            className="plume-open-project-manual-toggle"
            aria-expanded={manualOpen}
            onClick={() => setManualOpen((open) => !open)}
          >
            Enter path instead
          </button>
          {manualOpen ? (
            <form
              onSubmit={(event) => {
                event.preventDefault();
                if (canOpen) void submitCandidate(trimmed);
              }}
            >
              <label className="plume-open-form-label">
                Project path
                <input
                  type="text"
                  className="plume-open-form-input"
                  value={path}
                  placeholder="Paste a folder path"
                  spellCheck={false}
                  autoCapitalize="off"
                  autoCorrect="off"
                  disabled={unavailable}
                  onChange={(event) => setPath(event.target.value)}
                />
              </label>
              <button type="submit" className="ink-button" disabled={!canOpen}>
                {submitting ? 'Opening…' : 'Open'}
              </button>
            </form>
          ) : null}
        </div>
        {error ? <p className="plume-open-project-error" role="alert">{error}</p> : null}
      </div>
    </section>
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
  archivedContent,
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
  archivedContent: ReactNode;
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
              id: 'archived',
              label: 'Archived',
              description: 'Chats kept out of the sidebar.',
              content: archivedContent,
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
  archivedContent,
  onClose,
}: {
  inventory: ProviderInventory;
  servers: MlxServersApi;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
  appearance: ReturnType<typeof useAppearance>;
  archivedContent: ReactNode;
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
            {
              id: 'archived',
              label: 'Archived',
              description: 'Chats kept out of the sidebar.',
              content: archivedContent,
            },
          ]}
        />
      </div>
    </ModalDialog>
  );
}
