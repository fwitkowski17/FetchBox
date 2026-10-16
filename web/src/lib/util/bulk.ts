// What the actions on a selection of downloads apply to. Pure functions, no DOM.
import type { Download } from '../api/types';

const FINISHED = new Set(['completed', 'failed', 'cancelled']);
/** Not there yet: deleting them stops the transfer, and the partial file goes. (A download that is
 *  being verified has its whole file.) */
const TRANSFERRING = new Set(['queued', 'downloading', 'paused']);

/** Has a finished file on the disk: completed, being verified, or failed its verification. */
export function hasFinishedFile(d: Download): boolean {
  return d.status === 'completed' || d.status === 'verifying' || (d.status === 'failed' && d.verify_failed === true);
}

export interface BulkInfo {
  /** Selected downloads. */
  total: number;
  /** Finished, failed or cancelled and not archived yet: the ones "Archive" can take. */
  archivable: number;
  /** Already archived: the ones "Restore" can bring back. */
  archived: number;
  /** Not finished (queued, running, paused): deleting them stops them first. */
  unfinished: number;
  /** The ones that have a finished file on the disk (completed, verifying, failed verification). */
  withFile: number;
}

export function bulkInfo(items: readonly Download[]): BulkInfo {
  const info: BulkInfo = { total: items.length, archivable: 0, archived: 0, unfinished: 0, withFile: 0 };
  for (const d of items) {
    if (d.archived) info.archived++;
    else if (FINISHED.has(d.status)) info.archivable++;
    if (TRANSFERRING.has(d.status)) info.unfinished++;
    if (hasFinishedFile(d)) info.withFile++;
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
