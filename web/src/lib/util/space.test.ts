import { describe, expect, it } from 'vitest';
import { spaceNeed } from './space';

describe('spaceNeed', () => {
  it('shows how much of the file the free space covers and what is missing', () => {
    const n = spaceNeed(12_000, 0, 3_000);
    expect(n).toEqual({ needed: 12_000, free: 3_000, covered: 0.25, missing: 9_000, room: false });
  });

  it('counts only what is still to be written', () => {
    expect(spaceNeed(12_000, 10_000, 1_500).needed).toBe(2_000);
    expect(spaceNeed(12_000, 12_000, 0).room).toBe(true);
  });

  it('says there is room once the free space covers the need, exactly or more', () => {
    expect(spaceNeed(100, 0, 100)).toMatchObject({ room: true, missing: 0, covered: 1 });
    expect(spaceNeed(100, 0, 5_000)).toMatchObject({ room: true, missing: 0, covered: 1 });
    expect(spaceNeed(100, 0, 99)).toMatchObject({ room: false, missing: 1 });
  });

  it('copes with an unknown free space', () => {
    expect(spaceNeed(100, 0, null)).toEqual({
      needed: 100,
      free: null,
      covered: 0,
      missing: 100,
      room: false,
    });
  });
});
