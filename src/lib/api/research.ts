import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  ResearchCitationStatus,
  ResearchEventEnvelope,
  ResearchTerminalStatus,
} from './agentEvents';
import { invokeIpc } from './ipc';

export type { ResearchEventEnvelope } from './agentEvents';

const RESEARCH_EVENT_CHANNEL = 'research/event';

export type ResearchOwner = {
  scope: 'local' | 'project';
  sessionId: string;
};

export type ResearchSourceRef = {
  kind: 'browserTextEvidence';
  evidenceId: string;
};

export type ResearchStartPayload = {
  runId: string;
  owner: ResearchOwner;
  question: string;
  providerId: string;
  modelId: string;
  handleId?: string;
  sources: ResearchSourceRef[];
};

export type ResearchStartedResponse = {
  runId: string;
  providerId: string;
  modelId: string;
};

export type ResearchCancelResponse = {
  cancelled: boolean;
};

export type ResearchArtifactOutcome = ResearchTerminalStatus;

export type ResearchArtifactSummary = {
  artifactId: string;
  version: number;
  createdAtMs: number;
  question: string;
  providerId: string;
  modelId: string;
  citationStatus: ResearchCitationStatus;
  outcome: ResearchArtifactOutcome;
};

export type ResearchSourceView = {
  sourceId: string;
  evidenceId: string;
  sourceUrl: string;
  title: string | null;
  capturedAtMs: number;
  sha256: string;
  bytes: number;
  redactionCount: number;
  truncated: boolean;
};

export type ResearchLoadArtifactResponse = {
  artifact: ResearchArtifactSummary;
  markdown: string;
  sources: ResearchSourceView[];
  logicalTurns: number;
  providerCalls: number;
  durationMs: number;
};

export type ResearchExportOutcome =
  | { status: 'cancelled' }
  | { status: 'saved'; fileName: string };

export type ResearchEventHandler = (event: ResearchEventEnvelope) => void;

export function mintResearchRunId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `research-${crypto.randomUUID()}`;
  }
  return `research-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function startResearch(payload: ResearchStartPayload): Promise<ResearchStartedResponse> {
  return invokeIpc<ResearchStartPayload, ResearchStartedResponse>('research_start', payload);
}

export function cancelResearch(payload: {
  runId: string;
}): Promise<ResearchCancelResponse> {
  return invokeIpc<{ runId: string }, ResearchCancelResponse>('research_cancel', payload);
}

export function listResearchArtifacts(payload: {
  owner: ResearchOwner;
}): Promise<{ artifacts: ResearchArtifactSummary[] }> {
  return invokeIpc<
    { owner: ResearchOwner },
    { artifacts: ResearchArtifactSummary[] }
  >('research_list_artifacts', payload);
}

export function loadResearchArtifact(payload: {
  owner: ResearchOwner;
  artifactId: string;
  version?: number;
}): Promise<ResearchLoadArtifactResponse> {
  return invokeIpc<
    { owner: ResearchOwner; artifactId: string; version?: number },
    ResearchLoadArtifactResponse
  >('research_load_artifact', payload);
}

export function exportResearchArtifact(payload: {
  owner: ResearchOwner;
  artifactId: string;
  version: number;
}): Promise<ResearchExportOutcome> {
  return invokeIpc<
    { owner: ResearchOwner; artifactId: string; version: number },
    ResearchExportOutcome
  >('research_export_artifact', payload);
}

export async function subscribeResearchRun(
  runId: string,
  onEvent: ResearchEventHandler,
): Promise<UnlistenFn> {
  return listen<ResearchEventEnvelope>(RESEARCH_EVENT_CHANNEL, (event) => {
    if (event.payload.runId === runId) onEvent(event.payload);
  });
}
