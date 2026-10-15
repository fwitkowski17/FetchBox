// What the "waiting for space" gauge shows. Pure functions, no DOM.

export interface SpaceNeed {
  /** Bytes the download still has to write. */
  needed: number;
  /** Bytes free on the disk right now; `null` when the server cannot tell. */
  free: number | null;
  /** How much of the need is covered, 0..1 (0 when the free space is unknown). */
  covered: number;
  /** Bytes still missing (0 when there is room). */
  missing: number;
  /** The free space now covers the whole file. */
  room: boolean;
}

export function spaceNeed(size: number, downloaded: number, free: number | null): SpaceNeed {
  const needed = Math.max(0, size - downloaded);
  if (free === null) return { needed, free, covered: 0, missing: needed, room: false };
  const covered = needed > 0 ? Math.min(1, Math.max(0, free / needed)) : 1;
  return { needed, free, covered, missing: Math.max(0, needed - free), room: free >= needed };
}
