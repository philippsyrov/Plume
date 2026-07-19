import type { ReactNode } from 'react';

type SafeMarkdownPreviewProps = {
  markdown: string;
};

export function SafeMarkdownPreview({ markdown }: SafeMarkdownPreviewProps) {
  return <div className="plume-research-markdown">{parseBlocks(markdown)}</div>;
}

function parseBlocks(markdown: string): ReactNode[] {
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  const blocks: ReactNode[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index] ?? '';
    if (line.trim() === '') {
      index += 1;
      continue;
    }
    if (line.startsWith('```')) {
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !(lines[index] ?? '').startsWith('```')) {
        code.push(lines[index] ?? '');
        index += 1;
      }
      if (index < lines.length) index += 1;
      blocks.push(<pre key={`code-${index}`}><code>{code.join('\n')}</code></pre>);
      continue;
    }
    const heading = /^(#{1,6})\s+(.+)$/.exec(line);
    if (heading !== null) {
      const level = heading[1]?.length ?? 1;
      const content = inlineText(heading[2] ?? '');
      if (level === 1) blocks.push(<h1 key={`h-${index}`}>{content}</h1>);
      else if (level === 2) blocks.push(<h2 key={`h-${index}`}>{content}</h2>);
      else blocks.push(<h3 key={`h-${index}`}>{content}</h3>);
      index += 1;
      continue;
    }
    if (/^[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^[-*]\s+/.test(lines[index] ?? '')) {
        items.push((lines[index] ?? '').replace(/^[-*]\s+/, ''));
        index += 1;
      }
      blocks.push(
        <ul key={`list-${index}`}>
          {items.map((item, itemIndex) => <li key={itemIndex}>{inlineText(item)}</li>)}
        </ul>,
      );
      continue;
    }
    const paragraph: string[] = [line];
    index += 1;
    while (
      index < lines.length &&
      (lines[index] ?? '').trim() !== '' &&
      !/^(#{1,6})\s+/.test(lines[index] ?? '') &&
      !/^[-*]\s+/.test(lines[index] ?? '') &&
      !(lines[index] ?? '').startsWith('```')
    ) {
      paragraph.push(lines[index] ?? '');
      index += 1;
    }
    blocks.push(<p key={`p-${index}`}>{inlineText(paragraph.join(' '))}</p>);
  }
  return blocks;
}

function inlineText(value: string): string {
  return value
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, 'Remote image blocked: $1')
    .replace(/\[([^\]]+)\]\(<[^>]+>\)/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/[*_]{1,2}([^*_]+)[*_]{1,2}/g, '$1')
    .replace(/`([^`]+)`/g, '$1');
}
