import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { DiffBody, classifyDiffLine } from './DiffBody';

describe('classifyDiffLine', () => {
  it('routes file headers, hunks, adds, dels, and context', () => {
    expect(classifyDiffLine('--- a/x.txt')).toBe('header');
    expect(classifyDiffLine('+++ b/x.txt')).toBe('header');
    expect(classifyDiffLine('@@ -1 +1 @@')).toBe('hunk');
    expect(classifyDiffLine('+added')).toBe('add');
    expect(classifyDiffLine('-removed')).toBe('del');
    expect(classifyDiffLine(' context')).toBe('context');
  });
});

describe('DiffBody', () => {
  it('renders each line with an accessible label for adds and dels', () => {
    render(<DiffBody diff={'--- a/x.txt\n+++ b/x.txt\n@@ -1 +1 @@\n-a\n+b\n'} />);
    expect(screen.getByLabelText('Added: b')).toBeInTheDocument();
    expect(screen.getByLabelText('Removed: a')).toBeInTheDocument();
  });
});
