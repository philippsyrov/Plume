import type { ContextSourceRef } from '../../lib/api/chat';

export const PLUME_CONTEXT_MIME = 'application/x-plume-context-source+json';

export function writeContextDrag(
  dataTransfer: DataTransfer,
  source: ContextSourceRef,
): void {
  dataTransfer.setData(PLUME_CONTEXT_MIME, JSON.stringify(source));
  dataTransfer.effectAllowed = 'copy';
}

export function readContextDrop(dataTransfer: DataTransfer): ContextSourceRef | null {
  if (!Array.from(dataTransfer.types).includes(PLUME_CONTEXT_MIME)) return null;
  try {
    return validateContextSource(JSON.parse(dataTransfer.getData(PLUME_CONTEXT_MIME)));
  } catch {
    return null;
  }
}

function validateContextSource(value: unknown): ContextSourceRef | null {
  if (!isRecord(value) || typeof value.kind !== 'string') return null;
  if (value.kind === 'memoryEntry') return validateMemorySource(value);
  if (value.kind === 'topicFile') return validateTopicSource(value);
  if (value.kind === 'projectFile') return validateProjectFileSource(value);
  return null;
}

function validateMemorySource(value: Record<string, unknown>): ContextSourceRef | null {
  if (typeof value.entryId !== 'string' || !validMemoryId(value.entryId)) return null;
  return { kind: 'memoryEntry', entryId: value.entryId };
}

function validateTopicSource(value: Record<string, unknown>): ContextSourceRef | null {
  if (typeof value.name !== 'string' || !validTopicName(value.name)) return null;
  return { kind: 'topicFile', name: value.name };
}

function validateProjectFileSource(
  value: Record<string, unknown>,
): ContextSourceRef | null {
  if (typeof value.relPath !== 'string' || !validRelPath(value.relPath)) return null;
  const startLine = value.startLine;
  const endLine = value.endLine;
  if (startLine === undefined && endLine === undefined) {
    return { kind: 'projectFile', relPath: value.relPath };
  }
  if (!validLine(startLine) || !validLine(endLine) || startLine > endLine) return null;
  return { kind: 'projectFile', relPath: value.relPath, startLine, endLine };
}

function validMemoryId(id: string): boolean {
  return id.length === 34 && id.startsWith('m_') && /^[0-9a-fA-F]{32}$/.test(id.slice(2));
}

function validTopicName(name: string): boolean {
  const file = name.startsWith('topics/') ? name.slice('topics/'.length) : '';
  return (
    file !== '' &&
    file !== '.md' &&
    !file.startsWith('.') &&
    !file.includes('/') &&
    !file.includes('\\') &&
    file.endsWith('.md')
  );
}

function validRelPath(path: string): boolean {
  return (
    path.trim() !== '' &&
    path.length <= 1024 &&
    !path.startsWith('/') &&
    !path.startsWith('\\') &&
    !path.includes('\0') &&
    !path.split(/[\\/]/).some((component) => component === '..')
  );
}

function validLine(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value > 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
