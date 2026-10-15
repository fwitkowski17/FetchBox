import { describe, expect, it } from 'vitest';
import { retryDelay } from './retry';

describe('retry delay', () => {
  it('starts quick and settles', () => {
    expect([0, 1, 2, 3, 4, 5, 50].map(retryDelay)).toEqual([500, 1000, 2000, 4000, 5000, 5000, 5000]);
  });

  it('copes with nonsense', () => {
    expect(retryDelay(-3)).toBe(500);
    expect(retryDelay(1.9)).toBe(1000);
  });
});
