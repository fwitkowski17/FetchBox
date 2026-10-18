import { describe, expect, it } from 'vitest';
import type { DiskInfo, Disks } from '../api/types';
import { findDisk, folderOf, startingDisk, usedShare } from './disks';

const disk = (name: string, total = 1000, available = 400): DiskInfo => ({
  name,
  path: `/downloads/${name}`,
  total,
  available,
  reserved: 0,
});

function disks(over: Partial<Disks> = {}): Disks {
  return {
    disks: [disk(''), disk('hdd10tb'), disk('ssd1')],
    default: 'ssd1',
    default_available: true,
    choosable: true,
    ...over,
  };
}

describe('usedShare', () => {
  it('is the used part of the disk, kept within 0..1', () => {
    expect(usedShare({ total: 1000, available: 250 })).toBe(0.75);
    expect(usedShare({ total: 0, available: 0 })).toBe(0);
    expect(usedShare({ total: 100, available: 500 })).toBe(0);
  });
});

describe('startingDisk', () => {
  it('is the default disk while it can be used', () => {
    expect(startingDisk(disks())).toBe('ssd1');
  });

  it('falls back to the download folder when the default is gone or unknown', () => {
    expect(startingDisk(disks({ default_available: false }))).toBe('');
    expect(startingDisk(disks({ default: 'nvme9' }))).toBe('');
    expect(startingDisk(null)).toBe('');
  });

  it('is the download folder when that is the default', () => {
    expect(startingDisk(disks({ default: '' }))).toBe('');
  });
});

describe('findDisk', () => {
  it('finds a disk by name, the download folder by the empty name', () => {
    expect(findDisk(disks(), 'hdd10tb')?.path).toBe('/downloads/hdd10tb');
    expect(findDisk(disks(), '')?.name).toBe('');
    expect(findDisk(disks(), 'nope')).toBeUndefined();
    expect(findDisk(null, '')).toBeUndefined();
  });
});

describe('folderOf', () => {
  it('joins the disk and the category folder', () => {
    expect(folderOf({ disk: 'ssd1', subdir: 'video' })).toBe('ssd1/video/');
    expect(folderOf({ disk: 'ssd1', subdir: '' })).toBe('ssd1/');
    expect(folderOf({ disk: '', subdir: 'video' })).toBe('video/');
    expect(folderOf({ subdir: '' })).toBe('');
    expect(folderOf({})).toBe('');
  });
});
