import { describe, expect, it } from 'vitest';
import type { ListingEntry } from '../api/types';
import { emptySummary, selectionTotal, tally, visibleEntries } from './listing';

const file = (name: string, size?: number): ListingEntry => ({
  name,
  url: `http://h/${name}`,
  size,
  is_dir: false,
  category: 'other',
});
const dir = (name: string): ListingEntry => ({ name, url: `http://h/${name}/`, is_dir: true, category: 'other' });

describe('visibleEntries', () => {
  const all = [dir('docs'), file('Holiday.MP4'), file('notes.txt')];
  it('shows everything without a query', () => {
    expect(visibleEntries(all, '  ')).toHaveLength(3);
  });
  it('filters files by name, ignoring case, and hides folders', () => {
    expect(visibleEntries(all, 'holi').map((e) => e.name)).toEqual(['Holiday.MP4']);
    expect(visibleEntries(all, 'doc')).toEqual([]);
  });
});

describe('selectionTotal', () => {
  const entries = [dir('docs'), file('a', 100), file('b', 50), file('c')];
  it('sums the chosen files and flags unknown sizes', () => {
    const t = selectionTotal(entries, new Set(['http://h/a', 'http://h/c']));
    expect(t).toEqual({ count: 2, bytes: 100, sized: 1, partial: true });
  });
  it('is exact when every chosen size is known and ignores folders', () => {
    const t = selectionTotal(entries, new Set(['http://h/a', 'http://h/b', 'http://h/docs/']));
    expect(t).toEqual({ count: 2, bytes: 150, sized: 2, partial: false });
  });
  it('is empty for no selection', () => {
    expect(selectionTotal(entries, new Set())).toEqual({ count: 0, bytes: 0, sized: 0, partial: false });
  });
});

describe('tally', () => {
  it('counts outcomes and keeps the first failure message', () => {
    const s = emptySummary();
    tally(s, 'added');
    tally(s, 'existing');
    tally(s, 'failed', 'boom');
    tally(s, 'failed', 'later');
    expect(s).toEqual({ added: 1, existing: 1, noSpace: 0, failed: 2, firstError: 'boom' });
  });
});
