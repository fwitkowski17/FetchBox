// What a plugin asks for, as rows the install dialog can show, and what an update adds to it.
// The summary is worked out by the server from the manifest; this only arranges it for display.
import type { PermissionSummary } from '../api/types';

export const LIST_KINDS = [
  'programs',
  'download_hosts',
  'network',
  'webhook_hosts',
  'events',
  'api',
] as const;
export type ListKind = (typeof LIST_KINDS)[number];

export interface PermissionRow {
  kind: ListKind;
  items: string[];
  /** The items an update adds to what the installed version had (all of them when new). */
  added: string[];
}

export interface PermissionFlag {
  on: boolean;
  /** Newly asked for by an update. */
  added: boolean;
}

export interface PermissionView {
  rows: PermissionRow[];
  process: PermissionFlag;
  runsDownloads: PermissionFlag;
  /** Nothing is asked for at all. */
  none: boolean;
  /** An update asks for more than the installed version did. */
  widens: boolean;
}

/** Arrange `next` for display; `previous` (the installed version, on an update) marks what is new. */
export function permissionView(
  next: PermissionSummary,
  previous?: PermissionSummary | null,
): PermissionView {
  const rows: PermissionRow[] = [];
  for (const kind of LIST_KINDS) {
    const items = next[kind];
    if (items.length === 0) continue;
    const known = new Set(previous ? previous[kind] : []);
    rows.push({ kind, items, added: previous ? items.filter((i) => !known.has(i)) : [...items] });
  }
  const process = { on: next.process, added: next.process && !previous?.process };
  const runsDownloads = {
    on: next.runs_downloads,
    added: next.runs_downloads && !previous?.runs_downloads,
  };
  const widens =
    previous != null &&
    (rows.some((r) => r.added.length > 0) || process.added || runsDownloads.added);
  return { rows, process, runsDownloads, none: rows.length === 0 && !process.on && !runsDownloads.on, widens };
}
