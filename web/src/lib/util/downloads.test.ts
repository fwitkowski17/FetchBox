import { describe, expect, it } from 'vitest';
import type { AppEvent, Download } from '../api/types';
import {
  applyEvent,
  countCategories,
  countGroups,
  defaultDir,
  filterDownloads,
  groupOf,
  groupOfDownload,
  sortDownloads,
  summarize,
  type Filter,
} from './downloads';

function dl(id: number, over: Partial<Download> = {}): Download {
  return {
    id,
    url: `http://host/${id}`,
    filename: `file${id}.bin`,
    category: 'other',
    status: 'completed',
    downloaded: 0,
    source_kind: 'http',
    resumable: true,
    speed: 0,
    subdir: '',
    ...over,
  };
}

const all: Filter = { group: 'all', category: 'all', query: '' };
const ids = (items: Download[]) => items.map((d) => d.id);

describe('groups and filters', () => {
  it('maps statuses to tabs', () => {
    expect(groupOf('downloading')).toBe('active');
    expect(groupOf('queued')).toBe('active');
    expect(groupOf('paused')).toBe('paused');
    expect(groupOf('completed')).toBe('completed');
    expect(groupOf('failed')).toBe('stopped');
    expect(groupOf('cancelled')).toBe('stopped');
  });

  const items = [
    dl(1, { status: 'downloading', category: 'video', filename: 'Holiday.MKV' }),
    dl(2, { status: 'paused', category: 'music', filename: 'song.mp3' }),
    dl(3, { status: 'completed', category: 'video', filename: 'film.mkv', url: 'http://nas/film' }),
    dl(4, { status: 'failed', category: 'other' }),
  ];

  it('filters by group, category and text (name or url, case-insensitive)', () => {
    expect(ids(filterDownloads(items, { ...all, group: 'active' }))).toEqual([1]);
    expect(ids(filterDownloads(items, { ...all, category: 'video' }))).toEqual([1, 3]);
    expect(ids(filterDownloads(items, { ...all, query: ' holiday ' }))).toEqual([1]);
    expect(ids(filterDownloads(items, { ...all, query: 'NAS' }))).toEqual([3]);
    expect(ids(filterDownloads(items, { group: 'completed', category: 'video', query: 'film' }))).toEqual([3]);
  });

  it('counts each chip ignoring its own dimension', () => {
    const f: Filter = { group: 'completed', category: 'video', query: '' };
    expect(countGroups(items, f)).toEqual({ all: 2, active: 1, paused: 0, completed: 1, stopped: 0, archived: 0 });
    expect(countCategories(items, f)).toEqual({
      all: 1,
      photo: 0,
      music: 0,
      video: 1,
      program: 0,
      archive: 0,
      other: 0,
    });
  });
});

describe('sortDownloads', () => {
  it('sorts by date added with ids', () => {
    const items = [dl(2), dl(3), dl(1)];
    expect(ids(sortDownloads(items, { key: 'added', dir: 'desc' }))).toEqual([3, 2, 1]);
    expect(ids(sortDownloads(items, { key: 'added', dir: 'asc' }))).toEqual([1, 2, 3]);
  });

  it('orders names by the alphabet of the given language', () => {
    const items = [
      dl(1, { filename: 'zebra.zip' }),
      dl(2, { filename: 'ćma.zip' }),
      dl(3, { filename: 'cebula.zip' }),
      dl(4, { filename: 'dom.zip' }),
    ];
    const byName = (locale?: string) =>
      ids(sortDownloads(items, { key: 'name', dir: 'asc' }, locale));
    // Polish puts "ć" right after "c", before "d"; plain codepoint order would put it last.
    expect(byName('pl')).toEqual([3, 2, 4, 1]);
  });

  it('does not mutate its input', () => {
    const items = [dl(2), dl(1)];
    sortDownloads(items, { key: 'added', dir: 'asc' });
    expect(ids(items)).toEqual([2, 1]);
  });

  it('sorts names naturally and case-insensitively', () => {
    const items = [
      dl(1, { filename: 'b10.zip' }),
      dl(2, { filename: 'B2.zip' }),
      dl(3, { filename: 'a.zip' }),
    ];
    expect(ids(sortDownloads(items, { key: 'name', dir: 'asc' }))).toEqual([3, 2, 1]);
    expect(ids(sortDownloads(items, { key: 'name', dir: 'desc' }))).toEqual([1, 2, 3]);
  });

  it('keeps unknown sizes last in both directions', () => {
    const items = [dl(1, { size: 50 }), dl(2), dl(3, { size: 10 }), dl(4, { size: null })];
    expect(ids(sortDownloads(items, { key: 'size', dir: 'asc' }))).toEqual([3, 1, 4, 2]);
    expect(ids(sortDownloads(items, { key: 'size', dir: 'desc' }))).toEqual([1, 3, 4, 2]);
  });

  it('sorts by progress, unknown last', () => {
    const items = [
      dl(1, { size: 100, downloaded: 80 }),
      dl(2, { size: 100, downloaded: 10 }),
      dl(3, { downloaded: 5 }),
    ];
    expect(ids(sortDownloads(items, { key: 'progress', dir: 'desc' }))).toEqual([1, 2, 3]);
    expect(ids(sortDownloads(items, { key: 'progress', dir: 'asc' }))).toEqual([2, 1, 3]);
  });

  it('sorts by speed and by status importance', () => {
    const items = [
      dl(1, { status: 'completed' }),
      dl(2, { status: 'downloading', speed: 500 }),
      dl(3, { status: 'queued' }),
      dl(4, { status: 'downloading', speed: 900 }),
      dl(5, { status: 'paused' }),
    ];
    expect(ids(sortDownloads(items, { key: 'speed', dir: 'desc' }))[0]).toBe(4);
    expect(ids(sortDownloads(items, { key: 'status', dir: 'asc' }))).toEqual([4, 2, 3, 5, 1]);
  });

  it('breaks ties by newest id so the order is stable', () => {
    const items = [dl(1, { size: 5 }), dl(2, { size: 5 }), dl(3, { size: 5 })];
    expect(ids(sortDownloads(items, { key: 'size', dir: 'asc' }))).toEqual([3, 2, 1]);
  });

  it('suggests a natural direction per key', () => {
    expect(defaultDir('name')).toBe('asc');
    expect(defaultDir('status')).toBe('asc');
    expect(defaultDir('added')).toBe('desc');
    expect(defaultDir('size')).toBe('desc');
  });
});

describe('applyEvent', () => {
  const base = [dl(1, { status: 'downloading', size: 100, downloaded: 10 }), dl(2)];

  it('inserts new downloads at the top and replaces known ones', () => {
    const added = applyEvent(base, { type: 'download', download: dl(3) });
    expect(ids(added)).toEqual([3, 1, 2]);
    const changed = applyEvent(base, {
      type: 'download',
      download: dl(1, { status: 'paused', size: 100, downloaded: 40 }),
    });
    expect(ids(changed)).toEqual([1, 2]);
    expect(changed[0]?.status).toBe('paused');
    expect(base[0]?.status).toBe('downloading'); // input untouched
  });

  it('updates progress in place and learns the total size', () => {
    const ev: AppEvent = { type: 'progress', id: 1, downloaded: 60, speed: 123, total: 200 };
    const next = applyEvent(base, ev);
    expect(next[0]).toMatchObject({ downloaded: 60, speed: 123, size: 200, status: 'downloading' });
    // Unknown total keeps what we already know.
    const keep = applyEvent(base, { type: 'progress', id: 1, downloaded: 70, speed: 1, total: null });
    expect(keep[0]?.size).toBe(100);
  });

  it('ignores progress for unknown ids (list not loaded yet)', () => {
    expect(applyEvent(base, { type: 'progress', id: 99, downloaded: 1, speed: 1 })).toBe(base);
  });

  it('removes downloads and returns the same array when nothing changed', () => {
    expect(ids(applyEvent(base, { type: 'removed', id: 1 }))).toEqual([2]);
    expect(applyEvent(base, { type: 'removed', id: 42 })).toBe(base);
  });

  it('leaves the list alone for settings events', () => {
    const settings = {
      connections: 4,
      max_concurrent: 3,
      speed_per_download: 0,
      speed_global: 0,
    };
    expect(applyEvent(base, { type: 'settings', settings })).toBe(base);
  });
});

describe('summarize', () => {
  it('adds up the speed of running downloads only', () => {
    const items = [
      dl(1, { status: 'downloading', speed: 100 }),
      dl(2, { status: 'downloading', speed: 50 }),
      dl(3, { status: 'queued', speed: 999 }),
      dl(4, { status: 'paused', speed: 999 }),
    ];
    expect(summarize(items)).toEqual({ active: 2, queued: 1, speed: 150 });
  });
});

describe('archived downloads', () => {
  const items = [
    dl(1),
    dl(2, { archived: true }),
    dl(3, { status: 'failed', archived: true, category: 'music' }),
    dl(4, { status: 'downloading' }),
  ];

  it('are out of the main list and have their own tab', () => {
    expect(groupOfDownload(items[1] as Download)).toBe('archived');
    expect(groupOfDownload(items[3] as Download)).toBe('active');
    expect(ids(filterDownloads(items, all))).toEqual([1, 4]);
    expect(ids(filterDownloads(items, { ...all, group: 'archived' }))).toEqual([2, 3]);
    // The status tabs never show them.
    expect(ids(filterDownloads(items, { ...all, group: 'completed' }))).toEqual([1]);
    expect(ids(filterDownloads(items, { ...all, group: 'stopped' }))).toEqual([]);
  });

  it('are counted in their tab and not in "all"', () => {
    expect(countGroups(items, all)).toEqual({
      all: 2,
      active: 1,
      paused: 0,
      completed: 1,
      stopped: 0,
      archived: 2,
    });
    // Category and search still narrow every tab.
    expect(countGroups(items, { ...all, category: 'music' }).archived).toBe(1);
  });

  it('keep the category counts of the tab being viewed', () => {
    expect(countCategories(items, { ...all, group: 'archived' }).music).toBe(1);
    expect(countCategories(items, all).music).toBe(0);
  });
});
