function normalized(input: string): string {
  return input.trim().replace(/[.]$/, '').replace(/\s+/g, ' ').toLowerCase();
}

export function researchQuestion(input: string): string | null {
  const match = /^(?:please\s+|quickly\s+)?research\s+(.+)$/i.exec(input.trim());
  return match?.[1]?.trim() || null;
}

export function isMarkdownExportRequest(input: string): boolean {
  const value = normalized(input).replace(/^please\s+/, '');
  return value === 'export this as markdown' ||
    value === 'save this research as a markdown file';
}
