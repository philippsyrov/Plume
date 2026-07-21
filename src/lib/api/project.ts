// Typed wrapper for project IPC commands. Mirrors
// `docs/IPC_CONTRACT.md` § project. Translates the contract's dotted
// names (`project.open`) to flat Tauri command names (`project_open`)
// so the rest of the codebase stays in TS-idiomatic camelCase.

import { invokeIpc } from './ipc';

export type ProjectId = string;

export type PackageManager = 'npm' | 'pnpm' | 'yarn' | 'cargo' | 'pip';

export type GitState = {
  branch: string | null;
  dirtyCount: number;
};

export type TrustState = 'trusted' | 'unknown';

export type ProjectMeta = {
  id: ProjectId;
  root: string;
  hasAgentsMd: boolean;
  hasClaudeMd: boolean;
  packageManagers: PackageManager[];
  git: GitState | null;
  trust: TrustState;
};

type PathPayload = { path: string };
type EmptyPayload = Record<string, never>;
type TrustStateResponse = { trusted: boolean };

export function openProject(path: string): Promise<ProjectMeta> {
  return invokeIpc<PathPayload, ProjectMeta>('project_open', { path });
}

export function chooseProjectFolder(): Promise<string | null> {
  return invokeIpc<EmptyPayload, string | null>('project_choose_folder', {});
}

export function refreshProject(): Promise<ProjectMeta> {
  return invokeIpc<EmptyPayload, ProjectMeta>('project_refresh', {});
}

export function trustProject(path: string): Promise<ProjectMeta> {
  return invokeIpc<PathPayload, ProjectMeta>('project_trust', { path });
}

export function getTrustState(path: string): Promise<TrustStateResponse> {
  return invokeIpc<PathPayload, TrustStateResponse>('project_trust_state', { path });
}
