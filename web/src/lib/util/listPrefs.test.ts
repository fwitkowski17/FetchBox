import { describe, expect, it } from 'vitest';
import { DEFAULTS, readStored } from './listPrefs';

describe('readStored', () => {
  it('reads what the list saves now', () => {
    const saved = { sort: { key: 'size', dir: 'asc' }, group: 'paused', category: 'video', view: 'table' };
    expect(readStored(saved)).toEqual(saved);
  });

  it('keeps the sort and filters of a save from before the views, and shows cards', () => {
    const old = { sort: { key: 'name', dir: 'asc' }, group: 'completed', category: 'music' };
    expect(readStored(old)).toEqual({ ...old, view: 'cards' });
  });

  it('falls back to the defaults for anything else', () => {
    expect(readStored(null)).toEqual(DEFAULTS);
    expect(readStored('cards')).toEqual(DEFAULTS);
    expect(readStored({ ...DEFAULTS, view: 'grid' })).toEqual(DEFAULTS);
    expect(readStored({ sort: { key: 'nope', dir: 'asc' }, group: 'all', category: 'all' })).toEqual(DEFAULTS);
  });
});
