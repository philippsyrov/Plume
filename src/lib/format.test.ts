import { describe, expect, it } from 'vitest';

import { formatBytes, formatBytesOneDecimal } from './format';

describe('formatBytes', () => {
  it('renders sub-KiB counts as bytes', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1023 B');
  });

  it('renders the KiB tier at the 1024-byte boundary', () => {
    expect(formatBytes(1024)).toBe('1 KB');
    expect(formatBytes(1024 * 1024 - 1)).toBe('1024 KB');
  });

  it('renders the MiB tier at the 1024*1024-byte boundary', () => {
    expect(formatBytes(1024 * 1024)).toBe('1 MB');
    expect(formatBytes(1024 * 1024 * 1024 - 1)).toBe('1024 MB');
  });

  it('renders the GiB tier with one decimal place at the boundary', () => {
    expect(formatBytes(1024 * 1024 * 1024)).toBe('1.0 GB');
    expect(formatBytes(1024 * 1024 * 1024 * 2.5)).toBe('2.5 GB');
  });
});

describe('formatBytesOneDecimal', () => {
  it('renders sub-KiB counts as bytes', () => {
    expect(formatBytesOneDecimal(0)).toBe('0 B');
    expect(formatBytesOneDecimal(1023)).toBe('1023 B');
  });

  it('renders the KiB tier with one decimal place at the boundary', () => {
    expect(formatBytesOneDecimal(1024)).toBe('1.0 KB');
    expect(formatBytesOneDecimal(1536)).toBe('1.5 KB');
    expect(formatBytesOneDecimal(1024 * 1024 - 1)).toBe('1024.0 KB');
  });

  it('renders the MiB tier with one decimal place at the boundary, with no GiB tier', () => {
    expect(formatBytesOneDecimal(1024 * 1024)).toBe('1.0 MB');
    // A full GiB-sized value still renders as MB — this formatter has no
    // GB tier, unlike `formatBytes` above.
    expect(formatBytesOneDecimal(1024 * 1024 * 1024)).toBe('1024.0 MB');
  });
});
