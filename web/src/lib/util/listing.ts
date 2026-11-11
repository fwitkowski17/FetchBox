import type { ListingEntry } from '../api/types';

/** What the filter box shows: names containing the text (case-insensitive); folders stay visible
 *  only when nothing is typed, because the filter is about files. */
export function visibleEntries(entries: readonly ListingEntry[], query: string): ListingEntry[] {
  const q = query.trim().toLocaleLowerCase();
  if (q === '') return [...entries];
  return entries.filter((e) => !e.is_dir && e.name.toLocaleLowerCase().includes(q));
}

/** What identifies an entry in the chosen set. Presets of one video (made by a plugin) share the
 *  video's address, so the preset is part of the key; a plain file is its address. */
export function entryKey(e: ListingEntry): string {
  const via = e.via;
  return via ? `${e.url}\0${via.plugin}/${via.resolver}/${via.ref ?? ''}` : e.url;
}

export interface SelectionTotal {
  count: number;
  /** Sum of the sizes the page showed. */
  bytes: number;
  /** How many of the chosen files have a size on the page. */
  sized: number;
  /** Some chosen files have no size on the page, so `bytes` is a lower bound. */
  partial: boolean;
}

/** How many of `entries` are chosen and how big they are. */
export function selectionTotal(entries: readonly ListingEntry[], chosen: ReadonlySet<string>): SelectionTotal {
  let count = 0;
  let bytes = 0;
  let sized = 0;
  let partial = false;
  for (const e of entries) {
    if (e.is_dir || !chosen.has(entryKey(e))) continue;
    count += 1;
    if (e.size === null || e.size === undefined) partial = true;
    else {
      bytes += e.size;
      sized += 1;
    }
  }
  return { count, bytes, sized, partial };
}

export type Outcome = 'added' | 'existing' | 'noSpace' | 'failed';

export interface BatchSummary {
  added: number;
  existing: number;
  noSpace: number;
  failed: number;
  /** Message of the first failure, to show what went wrong. */
  firstError: string | undefined;
}

export function emptySummary(): BatchSummary {
  return { added: 0, existing: 0, noSpace: 0, failed: 0, firstError: undefined };
}

/** Count one more result of adding a chosen file. */
export function tally(summary: BatchSummary, outcome: Outcome, error?: string): void {
  summary[outcome] += 1;
  if (outcome === 'failed' && summary.firstError === undefined) summary.firstError = error;
}
