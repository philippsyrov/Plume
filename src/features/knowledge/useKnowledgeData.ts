import { useCallback, useEffect, useRef, useState } from 'react';

import {
  getMemoryIndex,
  getMemoryTopics,
  type MemoryIndex,
  type MemoryTopics,
} from '../../lib/api/memory';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { useMemoryRevision } from '../memory/memoryRevision';

export type KnowledgeSourceState<T> =
  | { kind: 'loading' }
  | { kind: 'ready'; data: T }
  | { kind: 'error'; message: string };

export type KnowledgeData = {
  memory: KnowledgeSourceState<MemoryIndex>;
  topics: KnowledgeSourceState<MemoryTopics>;
  retryMemory: () => void;
  retryTopics: () => void;
  refreshAll: () => void;
};

export function useKnowledgeData(): KnowledgeData {
  const revision = useMemoryRevision();
  const mounted = useRef(true);
  const memoryRequest = useRef(0);
  const topicRequest = useRef(0);
  const [memory, setMemory] = useState<KnowledgeSourceState<MemoryIndex>>({ kind: 'loading' });
  const [topics, setTopics] = useState<KnowledgeSourceState<MemoryTopics>>({ kind: 'loading' });

  const loadMemory = useCallback(() => {
    const request = ++memoryRequest.current;
    setMemory({ kind: 'loading' });
    void getMemoryIndex().then(
      (data) => {
        if (mounted.current && request === memoryRequest.current) {
          setMemory({ kind: 'ready', data });
        }
      },
      (error: unknown) => {
        if (mounted.current && request === memoryRequest.current) {
          setMemory({ kind: 'error', message: knowledgeError(error, 'memory entries') });
        }
      },
    );
  }, []);

  const loadTopics = useCallback(() => {
    const request = ++topicRequest.current;
    setTopics({ kind: 'loading' });
    void getMemoryTopics().then(
      (data) => {
        if (mounted.current && request === topicRequest.current) {
          setTopics({ kind: 'ready', data });
        }
      },
      (error: unknown) => {
        if (mounted.current && request === topicRequest.current) {
          setTopics({ kind: 'error', message: knowledgeError(error, 'memory topics') });
        }
      },
    );
  }, []);

  const refreshAll = useCallback(() => {
    loadMemory();
    loadTopics();
  }, [loadMemory, loadTopics]);

  useEffect(
    () => {
      mounted.current = true;
      return () => {
        mounted.current = false;
        memoryRequest.current += 1;
        topicRequest.current += 1;
      };
    },
    [],
  );

  useEffect(() => {
    refreshAll();
  }, [refreshAll, revision]);

  return {
    memory,
    topics,
    retryMemory: loadMemory,
    retryTopics: loadTopics,
    refreshAll,
  };
}

function knowledgeError(error: unknown, source: string): string {
  if (isIpcError(error)) {
    return error.kind === 'NeedsApproval'
      ? `Trust the project to read ${source}.`
      : ipcErrorMessage(error);
  }
  return error instanceof Error ? error.message : String(error);
}
