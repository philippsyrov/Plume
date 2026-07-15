import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { ICON_NAMES, Icon } from './Icon';

describe('Icon', () => {
  it('keeps the shared shell icon vocabulary closed and reviewable', () => {
    expect(ICON_NAMES).toEqual([
      'chat',
      'search',
      'library',
      'project',
      'files',
      'settings',
      'help',
      'sidebar-collapse',
      'sidebar-expand',
      'more',
      'plus',
      'close',
      'browser',
      'knowledge',
      'benchmarks',
      'terminal',
      'chevron-down',
      'arrow-left',
      'arrow-right',
      'reload',
      'expand',
      'contract',
    ]);
  });

  it('is decorative by default and inherits its owner color', () => {
    const { container } = render(<Icon name="search" />);
    const icon = container.querySelector('svg');

    expect(icon).toHaveAttribute('aria-hidden', 'true');
    expect(icon).toHaveAttribute('focusable', 'false');
    expect(icon).toHaveAttribute('stroke', 'currentColor');
    expect(screen.queryByRole('img')).not.toBeInTheDocument();
  });

  it('can expose an explicit accessible label when the owner needs one', () => {
    render(<Icon name="help" aria-label="Help" />);

    expect(screen.getByRole('img', { name: 'Help' })).not.toHaveAttribute(
      'aria-hidden',
    );
  });
});
