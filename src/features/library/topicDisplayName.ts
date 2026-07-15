import type { MemoryTopicFile } from '../../lib/api/memory';

export function topicDisplayName(file: MemoryTopicFile): string {
  if (file.kind === 'index') return 'Memory index';
  if (file.kind === 'user') return 'Project profile';
  if (file.kind === 'soul') return 'Assistant style';
  const filename = file.name.split('/').at(-1) ?? file.name;
  const words = filename.replace(/\.md$/i, '').replace(/[-_]+/g, ' ').trim();
  return words === '' ? 'Untitled topic' : words[0]!.toLocaleUpperCase() + words.slice(1);
}
