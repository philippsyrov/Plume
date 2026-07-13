import { lstatSync, readFileSync, realpathSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve } from 'node:path';

export type LinkIssueKind = 'missingFile' | 'missingAnchor' | 'pathEscape';

export type LinkIssue = {
  source: string;
  target: string;
  kind: LinkIssueKind;
  message: string;
};

type ParsedLink = {
  target: string;
};

function stripFencedCode(markdown: string): string {
  const output: string[] = [];
  let fence: { marker: '`' | '~'; length: number } | null = null;

  for (const line of markdown.split('\n')) {
    const opening = line.match(/^ {0,3}(`{3,}|~{3,})/);
    if (fence === null && opening?.[1] !== undefined) {
      fence = { marker: opening[1][0] as '`' | '~', length: opening[1].length };
      output.push('');
      continue;
    }

    if (fence !== null) {
      const closing = line.match(/^ {0,3}(`+|~+)\s*$/)?.[1];
      if (closing?.[0] === fence.marker && closing.length >= fence.length) fence = null;
      output.push('');
      continue;
    }

    output.push(line);
  }

  return output.join('\n');
}

function extractLinks(markdown: string): ParsedLink[] {
  const links: ParsedLink[] = [];
  const linkPattern = /!?\[[^\]]*\]\(\s*(?:<([^>]+)>|([^\s)]+))(?:\s+["'][^)]*["'])?\s*\)/g;

  for (const match of stripFencedCode(markdown).matchAll(linkPattern)) {
    const target = match[1] ?? match[2] ?? '';
    links.push({ target });
  }

  return links;
}

function githubSlug(value: string): string {
  return value
    .toLowerCase()
    .replace(/<[^>]*>/g, '')
    .replace(/!?(?:\[([^\]]+)\])\([^)]*\)/g, '$1')
    .replace(/[^\p{L}\p{M}\p{N}\s_-]/gu, '')
    .trim()
    .replace(/\s+/g, '-');
}

function headingAnchors(markdown: string): Set<string> {
  const anchors = new Set<string>();
  const duplicateCounts = new Map<string, number>();
  const lines = stripFencedCode(markdown).split('\n');
  const headings: string[] = [];

  for (let index = 0; index < lines.length; index += 1) {
    const atx = lines[index]?.match(/^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$/)?.[1];
    if (atx !== undefined) headings.push(atx);

    const nextLine = lines[index + 1];
    if (lines[index]?.trim() !== '' && nextLine !== undefined && /^ {0,3}(?:=+|-+)\s*$/.test(nextLine)) {
      headings.push(lines[index]!.trim());
      index += 1;
    }
  }

  for (const heading of headings) {
    const base = githubSlug(heading);
    let slug = base;
    while (anchors.has(slug)) {
      const nextCount = (duplicateCounts.get(base) ?? 0) + 1;
      duplicateCounts.set(base, nextCount);
      slug = `${base}-${nextCount}`;
    }
    anchors.add(slug);
  }

  return anchors;
}

function isOutsideRoot(root: string, candidate: string): boolean {
  const fromRoot = relative(root, candidate);
  return fromRoot === '..' || fromRoot.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) || isAbsolute(fromRoot);
}

function isFile(path: string): boolean {
  try {
    return lstatSync(path).isFile();
  } catch {
    return false;
  }
}

function compareIssues(left: LinkIssue, right: LinkIssue): number {
  const compare = (first: string, second: string): number => {
    const localeOrder = first.localeCompare(second, 'en', { ignorePunctuation: true });
    if (localeOrder !== 0) return localeOrder;
    if (first === second) return 0;
    return first < second ? -1 : 1;
  };
  return (
    compare(left.source, right.source) ||
    compare(left.target, right.target) ||
    compare(left.kind, right.kind)
  );
}

export function checkMarkdownLinks(root: string, relativeFiles: string[]): LinkIssue[] {
  const resolvedRoot = resolve(root);
  const canonicalRoot = realpathSync(resolvedRoot);
  const issues: LinkIssue[] = [];
  const markdownCache = new Map<string, string>();

  const readMarkdown = (path: string): string => {
    const cached = markdownCache.get(path);
    if (cached !== undefined) return cached;
    const markdown = readFileSync(path, 'utf8');
    markdownCache.set(path, markdown);
    return markdown;
  };

  for (const source of relativeFiles) {
    const sourcePath = resolve(resolvedRoot, source);
    const sourceMarkdown = readMarkdown(sourcePath);

    for (const { target } of extractLinks(sourceMarkdown)) {
      if (target === '' || /^(?:https?|mailto|data):/i.test(target)) continue;

      const hashIndex = target.indexOf('#');
      const rawPath = hashIndex === -1 ? target : target.slice(0, hashIndex);
      const rawAnchor = hashIndex === -1 ? '' : target.slice(hashIndex + 1);
      let decodedPath: string;
      let decodedAnchor: string;

      try {
        decodedPath = decodeURIComponent(rawPath.split('?')[0] ?? '');
        decodedAnchor = decodeURIComponent(rawAnchor);
      } catch {
        decodedPath = rawPath;
        decodedAnchor = rawAnchor;
      }

      const targetPath = decodedPath === '' ? sourcePath : resolve(dirname(sourcePath), decodedPath);
      if (isOutsideRoot(resolvedRoot, targetPath)) {
        issues.push({
          source,
          target,
          kind: 'pathEscape',
          message: `link target escapes the repository: ${target}`,
        });
        continue;
      }

      if (!isFile(targetPath)) {
        issues.push({
          source,
          target,
          kind: 'missingFile',
          message: `linked file does not exist: ${target}`,
        });
        continue;
      }

      const canonicalTarget = realpathSync(targetPath);
      if (isOutsideRoot(canonicalRoot, canonicalTarget)) {
        issues.push({
          source,
          target,
          kind: 'pathEscape',
          message: `link target escapes the repository: ${target}`,
        });
        continue;
      }

      if (decodedAnchor !== '' && !headingAnchors(readMarkdown(targetPath)).has(decodedAnchor)) {
        issues.push({
          source,
          target,
          kind: 'missingAnchor',
          message: `linked heading does not exist: ${target}`,
        });
      }
    }
  }

  return issues.sort(compareIssues);
}
