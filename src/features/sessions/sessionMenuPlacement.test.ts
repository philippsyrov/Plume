import { describe, expect, it } from 'vitest';

import { placeSessionMenu } from './sessionMenuPlacement';

const anchor = { left: 180, right: 220, top: 100, bottom: 124 };
const menu = { width: 132, height: 104 };

describe('placeSessionMenu', () => {
  it('places the menu below the anchor when the viewport has room', () => {
    expect(placeSessionMenu(anchor, menu, { width: 400, height: 400 })).toEqual({
      left: 88,
      top: 128,
    });
  });

  it('flips the menu above an anchor near the viewport bottom', () => {
    expect(
      placeSessionMenu({ ...anchor, top: 350, bottom: 374 }, menu, {
        width: 400,
        height: 400,
      }),
    ).toEqual({ left: 88, top: 242 });
  });

  it('keeps the menu inside the horizontal viewport inset', () => {
    expect(
      placeSessionMenu({ ...anchor, left: 2, right: 30 }, menu, {
        width: 200,
        height: 400,
      }),
    ).toEqual({ left: 8, top: 128 });
  });
});
