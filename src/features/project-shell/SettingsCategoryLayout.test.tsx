import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { SettingsCategoryLayout } from './SettingsCategoryLayout';

const categories = [
  { id: 'general', label: 'General', content: <p>Appearance controls</p> },
  { id: 'models', label: 'Models', content: <p>Model controls</p> },
  { id: 'personal', label: 'Personal', content: <p>About you controls</p> },
];

describe('SettingsCategoryLayout', () => {
  it('shows one calm settings page at a time behind stable category navigation', async () => {
    render(<SettingsCategoryLayout categories={categories} />);

    const navigation = screen.getByRole('navigation', { name: 'Settings sections' });
    expect(navigation).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'General' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('region', { name: 'General' })).toBeVisible();
    expect(document.getElementById('plume-settings-page-models')).not.toBeVisible();

    await userEvent.click(screen.getByRole('button', { name: 'Models' }));

    expect(screen.getByRole('button', { name: 'Models' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('region', { name: 'Models' })).toBeVisible();
    expect(document.getElementById('plume-settings-page-general')).not.toBeVisible();
  });

  it('keeps optional helper copy to one sentence on the owning page', () => {
    render(
      <SettingsCategoryLayout
        categories={[
          {
            id: 'models',
            label: 'Models',
            description: 'Choose what runs locally on this Mac.',
            content: <p>Model controls</p>,
          },
        ]}
      />,
    );

    expect(screen.getByText('Choose what runs locally on this Mac.')).toBeVisible();
  });
});
