// What the actions on a selection of downloads apply to. Pure functions, no DOM.
import type { Download } from '../api/types';

const FINISHED = new Set(['completed', 'failed', 'cancelled']);

export interface BulkInfo {
  /** Selected downloads. */
  total: number;
  /** Finished, failed or cancelled and not archived yet: the ones "Archive" can take. */
  archivable: number;
  /** Already archived: the ones "Restore" can bring back. */
  archived: number;
  /** Not finished (queued, running, paused): deleting them stops them first. */
  unfinished: number;
  /** Completed: the ones that have a file on the disk. */
  withFile: number;
}

export function bulkInfo(items: readonly Download[]): BulkInfo {
  const info: BulkInfo = { total: items.length, archivable: 0, archived: 0, unfinished: 0, withFile: 0 };
  for (const d of items) {
    if (d.archived) info.archived++;
    else if (FINISHED.has(d.status)) info.archivable++;
    if (!FINISHED.has(d.status)) info.unfinished++;
    if (d.status === 'completed') info.withFile++;
  }
  return info;
}

/** The selected downloads, in list order, ignoring ids that are gone. */
export function pickSelected(items: readonly Download[], ids: ReadonlySet<number>): Download[] {
  return items.filter((d) => ids.has(d.id));
}

/** What to do with a failed bulk run: how many worked and a message from the first failure. */
export interface BulkResult {
  done: number;
  failed: number;
  firstError: unknown;
}
