// Disks (SPEC 6.14): the places downloads can be saved to. Pure functions, no DOM.
import type { DiskInfo, DiskWarning, Disks } from '../api/types';

/** The i18n key of the sentence that explains a warning about a disk. */
export function warningKey(w: DiskWarning): 'disks.warn.inside_container' | 'disks.warn.host_system_disk' {
  return w === 'inside_container' ? 'disks.warn.inside_container' : 'disks.warn.host_system_disk';
}


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

/**
 * The disk the add-torrent dialog starts on: the default for torrents (their own, or else the
 * downloads' one) while it can be used, otherwise the download folder.
 */
export function startingTorrentDisk(disks: Disks | null, torrentDefault: string | null | undefined): string {
  if (!disks) return '';
  if (torrentDefault === null || torrentDefault === undefined) return startingDisk(disks);
  return findDisk(disks, torrentDefault) ? torrentDefault : '';
}

/** The folder shown for a download: the disk and the category folder, `ssd1/video/`; empty for none. */
export function folderOf(d: { disk?: string; subdir?: string }): string {
  const parts = [d.disk, d.subdir].filter((p): p is string => !!p);
  return parts.length > 0 ? `${parts.join('/')}/` : '';
}
