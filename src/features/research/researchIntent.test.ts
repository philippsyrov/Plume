import { describe, expect, it } from 'vitest';

import { isMarkdownExportRequest, researchQuestion } from './researchIntent';

describe('research chat intents', () => {
  it('extracts only an explicitly requested research question', () => {
    expect(researchQuestion('Research feathered dinosaurs.')).toBe('feathered dinosaurs.');
    expect(researchQuestion('Please research feathered dinosaurs.')).toBe('feathered dinosaurs.');
    expect(researchQuestion('Quickly research feathered dinosaurs.')).toBe('feathered dinosaurs.');
    expect(researchQuestion('Tell me about dinosaurs')).toBeNull();
    expect(researchQuestion('Research   ')).toBeNull();
  });

  it('accepts only the two explicit Markdown export requests', () => {
    expect(isMarkdownExportRequest('Export this as Markdown.')).toBe(true);
    expect(isMarkdownExportRequest('Please export this as markdown')).toBe(true);
    expect(isMarkdownExportRequest('Save this research as a Markdown file.')).toBe(true);
    expect(isMarkdownExportRequest('Export this as PDF')).toBe(false);
    expect(isMarkdownExportRequest('Could you export it?')).toBe(false);
  });
});
