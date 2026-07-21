import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

import bundledHandbook from '../../../docs/USER_GUIDE.md?raw';
import { ModalDialog } from '../project-shell/ModalDialog';

type HelpPanelProps = {
  handbook?: string;
  onClose: () => void;
};

type HandbookBlock =
  | { kind: 'heading'; level: number; text: string }
  | { kind: 'paragraph'; text: string }
  | { kind: 'list'; ordered: boolean; items: string[] }
  | { kind: 'note'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'table'; headers: string[]; rows: string[][] };

export function HelpPanel({ handbook = bundledHandbook, onClose }: HelpPanelProps) {
  const [fullGuideOpen, setFullGuideOpen] = useState(false);
  const blocks = useMemo(() => parseHandbook(handbook), [handbook]);
  const backButtonRef = useRef<HTMLButtonElement>(null);
  const handbookButtonRef = useRef<HTMLButtonElement>(null);
  const firstRenderRef = useRef(true);

  useEffect(() => {
    if (firstRenderRef.current) {
      firstRenderRef.current = false;
      return;
    }
    (fullGuideOpen ? backButtonRef.current : handbookButtonRef.current)?.focus();
  }, [fullGuideOpen]);

  return (
    <ModalDialog
      labelledBy="plume-help-title"
      className="plume-help-window"
      onClose={onClose}
    >
      <header className="plume-project-settings-header">
        <h3 id="plume-help-title">Help</h3>
        <button
          type="button"
          className="ink-button plume-project-settings-close"
          onClick={onClose}
          aria-label="Close help"
        >
          Close
        </button>
      </header>
      {fullGuideOpen ? (
        <div className="plume-project-settings-body plume-help-body plume-help-handbook">
          <button
            ref={backButtonRef}
            type="button"
            className="ink-button plume-help-back"
            onClick={() => setFullGuideOpen(false)}
          >
            Back to Help
          </button>
          <article aria-label="Plume Handbook">
            <HandbookDocument blocks={blocks} />
          </article>
        </div>
      ) : (
        <div className="plume-project-settings-body plume-help-body">
          <ul className="plume-help-topics" aria-label="Common help topics">
            <li>
              <h4>Chat or Project?</h4>
              <p>Chat answers without a project. Projects can use a trusted folder and reviewed actions.</p>
            </li>
            <li>
              <h4>Browser</h4>
              <p>Each task keeps its own Browser. Pages enter chat only when you attach them.</p>
            </li>
            <li>
              <h4>Library</h4>
              <p>About you stays on this Mac. Project memory stays with its trusted project.</p>
            </li>
            <li>
              <h4>Review changes</h4>
              <p>File changes show a diff before you choose Apply or Revert.</p>
            </li>
          </ul>
          <button
            ref={handbookButtonRef}
            type="button"
            className="ink-button plume-help-open-handbook"
            onClick={() => setFullGuideOpen(true)}
          >
            Open handbook
          </button>
        </div>
      )}
    </ModalDialog>
  );
}

function HandbookDocument({ blocks }: { blocks: HandbookBlock[] }) {
  return blocks.map((block, index) => {
    const key = `${block.kind}-${index}`;
    if (block.kind === 'heading') {
      const content = cleanInline(block.text);
      if (block.level === 1) return <h1 key={key}>{content}</h1>;
      if (block.level === 2) return <h2 key={key}>{content}</h2>;
      return <h3 key={key}>{content}</h3>;
    }
    if (block.kind === 'paragraph') return <p key={key}>{cleanInline(block.text)}</p>;
    if (block.kind === 'note') return <aside key={key}>{cleanInline(block.text)}</aside>;
    if (block.kind === 'code') return <pre key={key}><code>{block.text}</code></pre>;
    if (block.kind === 'table') {
      return (
        <table key={key} className="plume-help-table">
          <thead><tr>{block.headers.map((cell) => <th key={cell}>{cleanInline(cell)}</th>)}</tr></thead>
          <tbody>{block.rows.map((row, rowIndex) => (
            <tr key={`${key}-${rowIndex}`}>
              {row.map((cell, cellIndex) => <td key={`${key}-${rowIndex}-${cellIndex}`}>{cleanInline(cell)}</td>)}
            </tr>
          ))}</tbody>
        </table>
      );
    }
    const Tag = block.ordered ? 'ol' : 'ul';
    return (
      <Tag key={key}>
        {block.items.map((item, itemIndex) => (
          <li key={`${key}-${itemIndex}`}>{cleanInline(item)}</li>
        ))}
      </Tag>
    );
  });
}

function cleanInline(value: string): ReactNode {
  return value
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\\([\\`*_{}\[\]()#+\-.!])/g, '$1');
}

function parseHandbook(source: string): HandbookBlock[] {
  const lines = source.replace(/\r\n?/g, '\n').split('\n');
  const blocks: HandbookBlock[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index] ?? '';
    if (line.trim() === '' || /^---+$/.test(line.trim())) {
      index += 1;
      continue;
    }
    if (line.startsWith('```')) {
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index]?.startsWith('```')) {
        code.push(lines[index] ?? '');
        index += 1;
      }
      index += 1;
      blocks.push({ kind: 'code', text: code.join('\n') });
      continue;
    }
    const heading = /^(#{1,3})\s+(.+)$/.exec(line);
    if (heading?.[1] && heading[2]) {
      blocks.push({ kind: 'heading', level: heading[1].length, text: heading[2] });
      index += 1;
      continue;
    }
    if (line.startsWith('|')) {
      const table: string[] = [];
      while (index < lines.length && lines[index]?.startsWith('|')) {
        table.push(lines[index] ?? '');
        index += 1;
      }
      const parsedRows = table.map(parseTableRow);
      const headers = parsedRows[0] ?? [];
      const rows = parsedRows.slice(1).filter((row) => !row.every((cell) => /^:?-{3,}:?$/.test(cell)));
      blocks.push({ kind: 'table', headers, rows });
      continue;
    }
    if (/^>\s?/.test(line)) {
      const note: string[] = [];
      while (index < lines.length && /^>\s?/.test(lines[index] ?? '')) {
        note.push((lines[index] ?? '').replace(/^>\s?/, ''));
        index += 1;
      }
      blocks.push({ kind: 'note', text: note.join(' ') });
      continue;
    }
    const listMatch = /^(\s*)([-*]|\d+\.)\s+(.+)$/.exec(line);
    if (listMatch?.[2] && listMatch[3]) {
      const ordered = listMatch[2].endsWith('.');
      const items: string[] = [];
      while (index < lines.length) {
        const match = /^(\s*)([-*]|\d+\.)\s+(.+)$/.exec(lines[index] ?? '');
        if (!match?.[2] || !match[3] || match[2].endsWith('.') !== ordered) break;
        items.push(match[3]);
        index += 1;
      }
      blocks.push({ kind: 'list', ordered, items });
      continue;
    }
    const paragraph: string[] = [line.trim()];
    index += 1;
    while (index < lines.length && lines[index]?.trim() !== '') {
      const next = lines[index] ?? '';
      if (/^(#{1,3})\s+/.test(next) || next.startsWith('```') || next.startsWith('|') || /^>\s?/.test(next) || /^(\s*)([-*]|\d+\.)\s+/.test(next)) break;
      paragraph.push(next.trim());
      index += 1;
    }
    blocks.push({ kind: 'paragraph', text: paragraph.join(' ') });
  }

  return blocks;
}

function parseTableRow(line: string): string[] {
  return line.slice(1, line.endsWith('|') ? -1 : undefined).split('|').map((cell) => cell.trim());
}
