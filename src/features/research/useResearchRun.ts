import { useCallback, useEffect, useRef, useState } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  cancelResearch,
  listResearchArtifacts,
  loadResearchArtifact,
  mintResearchRunId,
  startResearch,
  subscribeResearchRun,
  type ResearchEventEnvelope,
  type ResearchLoadArtifactResponse,
  type ResearchOwner,
  type ResearchSourceRef,
} from '../../lib/api/research';
import type { ResearchPhase } from '../../lib/api/agentEvents';

export type ResearchRunStatus =
  | 'idle'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'complete'
  | 'needsReview'
  | 'stopped'
  | 'error';

export type ResearchStep = {
  phase: ResearchPhase;
  summary: string;
  current: number;
  total: number;
  state: 'active' | 'complete';
};

export type ResearchStartInput = {
  question: string;
  providerId: string;
  modelId: string;
  handleId?: string;
  sources: ResearchSourceRef[];
};

export type ResearchStartOutcome = 'started' | 'busy' | 'unavailable' | 'rejected';

export type ResearchRunApi = {
  status: ResearchRunStatus;
  activeRunId: string | null;
  steps: ResearchStep[];
  details: string[];
  artifact: ResearchLoadArtifactResponse | null;
  error: string | null;
  start: (input: ResearchStartInput) => Promise<ResearchStartOutcome>;
  stop: () => Promise<void>;
};

type ActiveRun = {
  runId: string;
  generation: number;
  ownerKey: string;
  expectedSeq: number;
  terminal: boolean;
  artifactId: string | null;
  artifactVersion: number | undefined;
  unlisten: UnlistenFn | null;
};

export function useResearchRun(owner: ResearchOwner | null): ResearchRunApi {
  const [status, setStatus] = useState<ResearchRunStatus>('idle');
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [steps, setSteps] = useState<ResearchStep[]>([]);
  const [details, setDetails] = useState<string[]>([]);
  const [artifact, setArtifact] = useState<ResearchLoadArtifactResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const activeRef = useRef<ActiveRun | null>(null);
  const generationRef = useRef(0);
  const mountedRef = useRef(true);
  const ownerRef = useRef(owner);
  ownerRef.current = owner;

  const ownerScope = owner?.scope ?? null;
  const ownerSessionId = owner?.sessionId ?? null;

  const handleEvent = useCallback(
    (
      event: ResearchEventEnvelope,
      generation: number,
      capturedOwner: ResearchOwner,
      capturedOwnerKey: string,
    ) => {
      const active = activeRef.current;
      if (
        active === null ||
        active.terminal ||
        active.runId !== event.runId ||
        active.generation !== generation ||
        active.ownerKey !== capturedOwnerKey
      ) {
        return;
      }
      if (event.seq < active.expectedSeq) return;
      if (event.seq > active.expectedSeq) {
        active.terminal = true;
        active.unlisten?.();
        activeRef.current = null;
        setActiveRunId(null);
        setStatus('error');
        setError('Research event sequence was incomplete. The run was stopped.');
        void cancelResearch({ runId: active.runId }).catch(() => undefined);
        return;
      }
      active.expectedSeq += 1;

      switch (event.kind) {
        case 'progress':
          setStatus('running');
          setSteps((current) => upsertStep(current, event));
          return;
        case 'recovery':
          setStatus('running');
          setDetails((current) => [...current, event.diagnostic]);
          return;
        case 'artifact':
          active.artifactId = event.artifactId;
          active.artifactVersion = event.artifactVersion;
          return;
        case 'terminal': {
          active.terminal = true;
          active.unlisten?.();
          activeRef.current = null;
          setActiveRunId(null);
          setSteps((current) => current.map((step) => ({ ...step, state: 'complete' })));
          if (event.status === 'stopped') {
            setStatus('stopped');
            setError(null);
            return;
          }
          if (event.status === 'failed') {
            setStatus('error');
            setError(event.diagnostic ?? 'Research could not finish.');
            return;
          }
          const artifactId = event.artifactId ?? active.artifactId;
          if (artifactId === null) {
            setStatus('error');
            setError('Research finished without an artifact.');
            return;
          }
          const terminalStatus = event.status === 'needsReview' ? 'needsReview' : 'complete';
          setStatus(terminalStatus);
          setError(null);
          void loadResearchArtifact({
            owner: capturedOwner,
            artifactId,
            ...(active.artifactId === artifactId && active.artifactVersion !== undefined
              ? { version: active.artifactVersion }
              : {}),
          })
            .then((loaded) => {
              if (
                mountedRef.current &&
                generationRef.current === generation &&
                ownerKey(ownerRef.current) === capturedOwnerKey
              ) {
                setArtifact(loaded);
                setStatus(
                  loaded.artifact.citationStatus === 'needsReview'
                    ? 'needsReview'
                    : 'complete',
                );
              }
            })
            .catch((loadError: unknown) => {
              if (
                mountedRef.current &&
                generationRef.current === generation &&
                ownerKey(ownerRef.current) === capturedOwnerKey
              ) {
                setStatus('error');
                setError(formatError(loadError));
              }
            });
        }
      }
    },
    [],
  );

  const start = useCallback(
    async (input: ResearchStartInput): Promise<ResearchStartOutcome> => {
      if (activeRef.current !== null) return 'busy';
      if (ownerScope === null || ownerSessionId === null) {
        setError('Save this chat before creating a research note.');
        setStatus('error');
        return 'unavailable';
      }
      const capturedOwner: ResearchOwner = {
        scope: ownerScope,
        sessionId: ownerSessionId,
      };
      const capturedOwnerKey = ownerKey(capturedOwner);
      const generation = generationRef.current + 1;
      generationRef.current = generation;
      const runId = mintResearchRunId();
      const active: ActiveRun = {
        runId,
        generation,
        ownerKey: capturedOwnerKey,
        expectedSeq: 0,
        terminal: false,
        artifactId: null,
        artifactVersion: undefined,
        unlisten: null,
      };
      activeRef.current = active;
      setActiveRunId(runId);
      setStatus('starting');
      setSteps([]);
      setDetails([]);
      setArtifact(null);
      setError(null);

      try {
        const unlisten = await subscribeResearchRun(runId, (event) => {
          handleEvent(event, generation, capturedOwner, capturedOwnerKey);
        });
        const current = activeRef.current;
        if (current !== active || generationRef.current !== generation) {
          unlisten();
          return 'unavailable';
        }
        active.unlisten = unlisten;
        const response = await startResearch({
          runId,
          owner: capturedOwner,
          ...input,
        });
        if (
          response.runId !== runId ||
          response.providerId !== input.providerId ||
          response.modelId !== input.modelId
        ) {
          throw new Error('Research start returned a mismatched identity.');
        }
        if (activeRef.current === active) setStatus('running');
        return 'started';
      } catch (startError) {
        if (activeRef.current === active) {
          active.terminal = true;
          active.unlisten?.();
          activeRef.current = null;
          setActiveRunId(null);
          setStatus('error');
          setError(formatError(startError));
          void cancelResearch({ runId }).catch(() => undefined);
        }
        return 'rejected';
      }
    },
    [handleEvent, ownerScope, ownerSessionId],
  );

  const stop = useCallback(async () => {
    const active = activeRef.current;
    if (active === null || active.terminal) return;
    setStatus('stopping');
    try {
      await cancelResearch({ runId: active.runId });
    } catch (cancelError) {
      if (activeRef.current === active) {
        setStatus('running');
        setError(formatError(cancelError));
      }
    }
  }, []);

  useEffect(() => {
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    const active = activeRef.current;
    if (active !== null) {
      active.terminal = true;
      active.unlisten?.();
      activeRef.current = null;
      void cancelResearch({ runId: active.runId }).catch(() => undefined);
    }
    setActiveRunId(null);
    setStatus('idle');
    setSteps([]);
    setDetails([]);
    setArtifact(null);
    setError(null);
    if (ownerScope === null || ownerSessionId === null) return;

    const capturedOwner: ResearchOwner = {
      scope: ownerScope,
      sessionId: ownerSessionId,
    };
    const capturedOwnerKey = ownerKey(capturedOwner);
    void listResearchArtifacts({ owner: capturedOwner })
      .then(({ artifacts }) => {
        if (
          !mountedRef.current ||
          generationRef.current !== generation ||
          ownerKey(ownerRef.current) !== capturedOwnerKey ||
          artifacts.length === 0
        ) {
          return null;
        }
        const latest = [...artifacts].sort(
          (left, right) =>
            right.createdAtMs - left.createdAtMs || right.version - left.version,
        )[0];
        return loadResearchArtifact({
          owner: capturedOwner,
          artifactId: latest.artifactId,
          version: latest.version,
        });
      })
      .then((loaded) => {
        if (
          loaded !== null &&
          mountedRef.current &&
          generationRef.current === generation &&
          ownerKey(ownerRef.current) === capturedOwnerKey
        ) {
          setArtifact(loaded);
          setStatus(
            loaded.artifact.citationStatus === 'needsReview' ? 'needsReview' : 'complete',
          );
        }
      })
      .catch((loadError: unknown) => {
        if (
          mountedRef.current &&
          generationRef.current === generation &&
          ownerKey(ownerRef.current) === capturedOwnerKey
        ) {
          setStatus('error');
          setError(formatError(loadError));
        }
      });
  }, [ownerScope, ownerSessionId]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
      const active = activeRef.current;
      if (active !== null) {
        active.terminal = true;
        active.unlisten?.();
        activeRef.current = null;
        void cancelResearch({ runId: active.runId }).catch(() => undefined);
      }
    };
  }, []);

  return { status, activeRunId, steps, details, artifact, error, start, stop };
}

function upsertStep(
  steps: ResearchStep[],
  event: Extract<ResearchEventEnvelope, { kind: 'progress' }>,
): ResearchStep[] {
  const next: ResearchStep[] = steps.map((step) => ({ ...step, state: 'complete' }));
  const value: ResearchStep = {
    phase: event.phase,
    summary: event.summary,
    current: event.current,
    total: event.total,
    state: 'active',
  };
  const index = next.findIndex((step) => step.phase === event.phase);
  if (index === -1) return [...next, value];
  next[index] = value;
  return next;
}

function ownerKey(owner: ResearchOwner): string;
function ownerKey(owner: null): null;
function ownerKey(owner: ResearchOwner | null): string | null;
function ownerKey(owner: ResearchOwner | null): string | null {
  return owner === null ? null : `${owner.scope}:${owner.sessionId}`;
}

function formatError(error: unknown): string {
  if (isIpcError(error)) return ipcErrorMessage(error);
  if (error instanceof Error) return error.message;
  return 'Research could not finish.';
}
