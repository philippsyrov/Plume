import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { AppearancePanel } from './AppearancePanel';

describe('AppearancePanel', () => {
  it('offers System, Light, and Dark as one clear Settings choice', async () => {
    const onChange = vi.fn();
    render(<AppearancePanel value="light" onChange={onChange} />);

    expect(screen.getByRole('group', { name: 'Appearance' })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Light' })).toBeChecked();
    await userEvent.click(screen.getByRole('radio', { name: 'Dark' }));
    expect(onChange).toHaveBeenCalledWith('dark');
    expect(screen.getByText('Custom colors are planned for later.')).toBeInTheDocument();
  });
});
