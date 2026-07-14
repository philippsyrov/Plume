import type { ContextSourceRef } from '../../lib/api/chat';

export const MAX_CONTEXT_SOURCES = 16;

export type AddContextSourceResult = 'added' | 'duplicate' | 'full' | 'unavailable';

export function normalizeContextSources(sources: ContextSourceRef[]): ContextSourceRef[] {
  return sources.map((source) => {
    if (source.kind !== 'projectFile') return source;
    if (typeof source.startLine === 'number' && typeof source.endLine === 'number') {
      return source;
    }
    return { kind: 'projectFile', relPath: source.relPath };
  });
}

export function contextSourceKey(source: ContextSourceRef): string {
  switch (source.kind) {
    case 'projectFile':
      return `file:${source.relPath}:${source.startLine ?? ''}:${source.endLine ?? ''}`;
    case 'memoryEntry':
      return `memory:${source.entryId}`;
    case 'topicFile':
      return `topic:${source.name}`;
    case 'browserTextEvidence':
      return `browser-text:${source.evidenceId}`;
  }
}

export function addContextSourceToList(
  sources: ContextSourceRef[],
  source: ContextSourceRef,
): { result: AddContextSourceResult; sources: ContextSourceRef[] } {
  const key = contextSourceKey(source);
  if (sources.some((item) => contextSourceKey(item) === key)) {
    return { result: 'duplicate', sources };
  }
  if (sources.length >= MAX_CONTEXT_SOURCES) {
    return { result: 'full', sources };
  }
  return { result: 'added', sources: [...sources, source] };
}

export function removeContextSourceFromList(
  sources: ContextSourceRef[],
  source: ContextSourceRef,
): ContextSourceRef[] {
  const key = contextSourceKey(source);
  return sources.filter((item) => contextSourceKey(item) !== key);
}

export function sameContextSources(a: ContextSourceRef[], b: ContextSourceRef[]): boolean {
  return a.length === b.length && a.every((source, index) => contextSourceKey(source) === contextSourceKey(b[index]));
}
