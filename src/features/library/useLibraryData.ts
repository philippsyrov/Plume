import { useCallback, useEffect, useRef, useState } from 'react';

import {
  getMemoryIndex,
  getMemoryTopics,
  getUserMemoryIndex,
  type MemoryIndex,
  type MemoryTopics,
  type UserMemoryIndex,
} from '../../lib/api/memory';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { useMemoryRevision } from '../memory/memoryRevision';
import { useUserMemoryRevision } from './libraryRevision';
import type { LibraryData, LibrarySourceState } from './libraryTypes';
import { newestUserMemoryFirst } from './userMemoryOrder';

type ScopedSource<T> = {
  projectIdentity: string | null;
  value: LibrarySourceState<T>;
};

export function useLibraryData({
  projectIdentity,
}: {
  projectIdentity: string | null;
}): LibraryData {
  const projectRevision = useMemoryRevision();
  const userRevision = useUserMemoryRevision();
  const mounted = useRef(true);
  const userRequest = useRef(0);
  const projectRequest = useRef(0);
  const topicRequest = useRef(0);
  const projectIdentityRef = useRef(projectIdentity);
  projectIdentityRef.current = projectIdentity;
  const [userMemory, setUserMemory] = useState<LibrarySourceState<UserMemoryIndex>>({
    kind: 'loading',
  });
  const [projectMemoryState, setProjectMemoryState] = useState<ScopedSource<MemoryIndex>>({
    projectIdentity,
    value: projectIdentity === null ? { kind: 'unavailable' } : { kind: 'loading' },
  });
  const [topicState, setTopicState] = useState<ScopedSource<MemoryTopics>>({
    projectIdentity,
    value: projectIdentity === null ? { kind: 'unavailable' } : { kind: 'loading' },
  });
  const projectMemory = projectMemoryState.projectIdentity === projectIdentity
    ? projectMemoryState.value
    : projectIdentity === null
      ? { kind: 'unavailable' as const }
      : { kind: 'loading' as const };
  const topics = topicState.projectIdentity === projectIdentity
    ? topicState.value
    : projectIdentity === null
      ? { kind: 'unavailable' as const }
      : { kind: 'loading' as const };

  const loadUserMemory = useCallback(() => {
    const request = ++userRequest.current;
    setUserMemory({ kind: 'loading' });
    void getUserMemoryIndex().then(
      (data) => {
        if (mounted.current && request === userRequest.current) {
          setUserMemory({ kind: 'ready', data: newestUserMemoryFirst(data) });
        }
      },
      (error: unknown) => {
        if (mounted.current && request === userRequest.current) {
          setUserMemory({ kind: 'error', message: libraryError(error, 'About you') });
        }
      },
    );
  }, []);

  const loadProjectMemory = useCallback(() => {
    const identity = projectIdentityRef.current;
    const request = ++projectRequest.current;
    if (identity === null) {
      setProjectMemoryState({ projectIdentity: null, value: { kind: 'unavailable' } });
      return;
    }
    setProjectMemoryState({ projectIdentity: identity, value: { kind: 'loading' } });
    void getMemoryIndex().then(
      (data) => {
        if (
          mounted.current &&
          request === projectRequest.current &&
          identity === projectIdentityRef.current
        ) {
          setProjectMemoryState({
            projectIdentity: identity,
            value: { kind: 'ready', data },
          });
        }
      },
      (error: unknown) => {
        if (
          mounted.current &&
          request === projectRequest.current &&
          identity === projectIdentityRef.current
        ) {
          setProjectMemoryState({
            projectIdentity: identity,
            value: { kind: 'error', message: libraryError(error, 'project memory') },
          });
        }
      },
    );
  }, []);

  const loadTopics = useCallback(() => {
    const identity = projectIdentityRef.current;
    const request = ++topicRequest.current;
    if (identity === null) {
      setTopicState({ projectIdentity: null, value: { kind: 'unavailable' } });
      return;
    }
    setTopicState({ projectIdentity: identity, value: { kind: 'loading' } });
    void getMemoryTopics().then(
      (data) => {
        if (
          mounted.current &&
          request === topicRequest.current &&
          identity === projectIdentityRef.current
        ) {
          setTopicState({
            projectIdentity: identity,
            value: { kind: 'ready', data },
          });
        }
      },
      (error: unknown) => {
        if (
          mounted.current &&
          request === topicRequest.current &&
          identity === projectIdentityRef.current
        ) {
          setTopicState({
            projectIdentity: identity,
            value: { kind: 'error', message: libraryError(error, 'topics') },
          });
        }
      },
    );
  }, []);

  const refreshAll = useCallback(() => {
    loadUserMemory();
    loadProjectMemory();
    loadTopics();
  }, [loadProjectMemory, loadTopics, loadUserMemory]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      userRequest.current += 1;
      projectRequest.current += 1;
      topicRequest.current += 1;
    };
  }, []);

  useEffect(() => {
    loadUserMemory();
  }, [loadUserMemory, userRevision]);

  useEffect(() => {
    projectRequest.current += 1;
    topicRequest.current += 1;
    if (projectIdentity === null) {
      setProjectMemoryState({ projectIdentity: null, value: { kind: 'unavailable' } });
      setTopicState({ projectIdentity: null, value: { kind: 'unavailable' } });
      return;
    }
    setProjectMemoryState({ projectIdentity, value: { kind: 'loading' } });
    setTopicState({ projectIdentity, value: { kind: 'loading' } });
    loadProjectMemory();
    loadTopics();
  }, [loadProjectMemory, loadTopics, projectIdentity, projectRevision]);

  return {
    userMemory,
    projectMemory,
    topics,
    retryUserMemory: loadUserMemory,
    retryProjectMemory: loadProjectMemory,
    retryTopics: loadTopics,
    refreshAll,
  };
}

function libraryError(error: unknown, source: string): string {
  if (isIpcError(error)) {
    return error.kind === 'NeedsApproval'
      ? `Trust the project to read ${source}.`
      : ipcErrorMessage(error);
  }
  return error instanceof Error ? error.message : String(error);
}
