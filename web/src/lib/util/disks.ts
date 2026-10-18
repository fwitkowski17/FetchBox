// Disks (SPEC 6.14): the places downloads can be saved to. Pure functions, no DOM.
import type { DiskInfo, Disks } from '../api/types';

/** The share of a disk that is in use, 0..1. */
export function usedShare(d: Pick<DiskInfo, 'total' | 'available'>): number {
  if (d.total <= 0) return 0;
  return Math.min(1, Math.max(0, (d.total - d.available) / d.total));
}

/** The disk with this name, if the server listed it. */
export function findDisk(disks: Disks | null, name: string): DiskInfo | undefined {
  return disks?.disks.find((d) => d.name === name);
}

/**
 * The disk the add dialog starts on: the default one when it can be used now, otherwise the
 * download directory itself (which is always there). The server refuses a request that names a
 * disk that is gone, so offering one would only end in an error.
 */
export function startingDisk(disks: Disks | null): string {
  if (!disks) return '';
  return disks.default_available && findDisk(disks, disks.default) ? disks.default : '';
}

/** The folder shown for a download: the disk and the category folder, `ssd1/video/`; empty for none. */
export function folderOf(d: { disk?: string; subdir?: string }): string {
  const parts = [d.disk, d.subdir].filter((p): p is string => !!p);
  return parts.length > 0 ? `${parts.join('/')}/` : '';
}
