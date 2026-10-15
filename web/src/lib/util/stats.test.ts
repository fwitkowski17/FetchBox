import { describe, expect, it } from 'vitest';
import { barShare, niceAxis, successPercent, trimAxisZero } from './stats';

const MB = 1024 ** 2;
const GB = 1024 ** 3;

describe('niceAxis', () => {
  it('rounds the top up to 1, 2, 5, 10... of the unit it is written in', () => {
    expect(niceAxis(300).top).toBe(500);
    expect(niceAxis(1.3 * MB).top).toBe(2 * MB);
    expect(niceAxis(3.2 * MB).top).toBe(5 * MB);
    expect(niceAxis(7 * MB).top).toBe(10 * MB);
    expect(niceAxis(1.1 * GB).top).toBe(2 * GB);
  });

  it('never goes below the data, and an exact fit stays', () => {
    for (const max of [1, 17, 999, 1024, 5 * MB, 700 * MB, 3 * GB, 1_000_000_000_000]) {
      expect(niceAxis(max).top, String(max)).toBeGreaterThanOrEqual(max);
    }
    expect(niceAxis(5 * MB).top).toBe(5 * MB);
  });

  it('has three ticks: zero, half, top', () => {
    expect(niceAxis(3 * MB).ticks).toEqual([0, 2.5 * MB, 5 * MB]);
  });

  it('does not stay at 1000 of a unit that has a bigger one', () => {
    // 600 MB fits "1000 MB" (a clean label) and does not jump to a 1 GB axis for no reason.
    expect(niceAxis(600 * MB).top).toBe(1000 * MB);
  });

  it('gives an empty chart a usable axis', () => {
    expect(niceAxis(0).top).toBe(MB);
    expect(niceAxis(Number.NaN).top).toBe(MB);
  });
});

describe('barShare', () => {
  it('scales and clamps', () => {
    expect(barShare(50, 200)).toBe(0.25);
    expect(barShare(500, 200)).toBe(1);
  });

  it('draws nothing for nothing and something for anything', () => {
    expect(barShare(0, 100)).toBe(0);
    expect(barShare(1, 1_000_000)).toBeGreaterThan(0);
    expect(barShare(5, 0)).toBe(0);
  });
});

describe('successPercent', () => {
  it('is the share of completed among completed and failed', () => {
    expect(successPercent(9, 1)).toBe(90);
    expect(successPercent(1, 2)).toBe(33);
    expect(successPercent(5, 0)).toBe(100);
    expect(successPercent(0, 3)).toBe(0);
  });

  it('is unknown before anything ended', () => {
    expect(successPercent(0, 0)).toBeNull();
  });
});

describe('trimAxisZero', () => {
  it('drops a zero decimal and nothing else', () => {
    expect(trimAxisZero('5.0 GB')).toBe('5 GB');
    expect(trimAxisZero('5,0 GB')).toBe('5 GB');
    expect(trimAxisZero('2.5 GB')).toBe('2.5 GB');
    expect(trimAxisZero('10 GB')).toBe('10 GB');
    expect(trimAxisZero('500 KB')).toBe('500 KB');
    expect(trimAxisZero('0')).toBe('0');
    expect(trimAxisZero('1.05 GB')).toBe('1.05 GB');
  });
});
