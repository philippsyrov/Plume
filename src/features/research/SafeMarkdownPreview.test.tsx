import { render, screen } from '@testing-library/react';
import { expect, it } from 'vitest';

import { SafeMarkdownPreview } from './SafeMarkdownPreview';

it('renders safe blocks, blocks images, and leaves links as inert text', () => {
  const { container } = render(
    <SafeMarkdownPreview
      markdown={`# Note

Read [the source](https://example.com). [^S1]

- First item
- Second item

![tracking pixel](https://example.com/pixel.png)

\`\`\`ts
const safe = true;
\`\`\`

[^S1]: [Example](<https://example.com>)`}
    />,
  );

  expect(screen.getByRole('heading', { name: 'Note' })).toBeVisible();
  expect(screen.getByText(/Read the source/)).toBeVisible();
  expect(screen.getByText('Remote image blocked: tracking pixel')).toBeVisible();
  expect(screen.getByText('const safe = true;')).toBeVisible();
  expect(container.querySelector('a')).toBeNull();
  expect(container.querySelector('img')).toBeNull();
  expect(container.querySelector('script')).toBeNull();
});

it('renders model-supplied HTML as text instead of executing it', () => {
  const { container } = render(
    <SafeMarkdownPreview markdown={'<script>window.bad = true</script> [[S1]]'} />,
  );

  expect(screen.getByText(/<script>window\.bad = true<\/script>/)).toBeVisible();
  expect(container.querySelector('script')).toBeNull();
});
