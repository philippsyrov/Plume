import type { UserMemoryIndex } from '../../lib/api/memory';

export function newestUserMemoryFirst(index: UserMemoryIndex): UserMemoryIndex {
  return {
    ...index,
    entries: [...index.entries].sort(
      (left, right) => right.createdMs - left.createdMs || left.id.localeCompare(right.id),
    ),
  };
}
