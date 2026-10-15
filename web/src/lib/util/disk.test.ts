import { describe, expect, it } from 'vitest';
import type { DiskSpace } from '../api/types';
import { diskView } from './disk';

const GB = 1024 ** 3;
const disk = (usedGb: number, totalGb: number, reservedGb = 0): DiskSpace => ({
  total: totalGb * GB,
  available: (totalGb - usedGb) * GB,
  reserved: reservedGb * GB,
});

describe('diskView', () => {
  it('computes the used and reserved shares', () => {
    const v = diskView(disk(50, 100, 20));
    expect(v.used).toBeCloseTo(0.5);
    expect(v.reserved).toBeCloseTo(0.2);
    expect(v.short).toBe(0);
    expect(v.level).toBe('ok');
  });

  it('turns amber at 85 % and red at 95 %', () => {
    expect(diskView(disk(84, 100)).level).toBe('ok');
    expect(diskView(disk(85, 100)).level).toBe('warn');
    expect(diskView(disk(94, 100)).level).toBe('warn');
    expect(diskView(disk(95, 100)).level).toBe('full');
  });

  it('is red when the queue does not fit, whatever the usage, and says by how much', () => {
    const v = diskView(disk(10, 100, 95));
    expect(v.level).toBe('full');
    expect(v.short).toBe(5 * GB);
    // The reserved arc stops at the end of the free space: it cannot leave the ring.
    expect(v.used + v.reserved).toBeCloseTo(1);
  });

  it('an exact fit is not a shortfall', () => {
    const v = diskView(disk(10, 100, 90));
    expect(v.short).toBe(0);
    expect(v.level).toBe('ok');
  });

  it('survives an empty or inconsistent report', () => {
    expect(diskView({ total: 0, available: 0, reserved: 0 })).toEqual({
      used: 0,
      reserved: 0,
      level: 'ok',
      short: 0,
    });
    const odd = diskView({ total: 10, available: 20, reserved: 0 });
    expect(odd.used).toBe(0);
  });
});
