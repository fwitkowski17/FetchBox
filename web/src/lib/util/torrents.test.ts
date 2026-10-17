import { describe, expect, it } from 'vitest';
import type { Torrent } from '../api/types';
import {
  HISTORY_MS,
  applyTorrentEvent,
  chartPoints,
  filterTorrents,
  isActive,
  niceMax,
  primaryAction,
  progressOf,
  pushSample,
  ratioOf,
  sortTorrents,
  summarizeTorrents,
} from './torrents';

function torrent(over: Partial<Torrent> = {}): Torrent {
  return {
    id: 1,
    info_hash: 'ab'.repeat(20),
    name: 'Ubuntu',
    status: 'downloading',
    size: 1000,
    downloaded: 250,
    uploaded: 100,
    down_speed: 0,
    up_speed: 0,
    eta: null,
    peers_connected: 0,
    peers_connecting: 0,
    peers_known: 0,
    file_count: 1,
    magnet: null,
    added_at: 0,
    finished_at: null,
    error: null,
    ...over,
  };
}

describe('progress and ratio', () => {
  it('is a fraction of the size, unknown without one', () => {
    expect(progressOf(torrent())).toBe(0.25);
    expect(progressOf(torrent({ size: null }))).toBeNull();
    expect(progressOf(torrent({ size: 0 }))).toBe(1);
    expect(progressOf(torrent({ downloaded: 5000 }))).toBe(1);
  });
  it('is what was sent over what was received', () => {
    expect(ratioOf(torrent({ downloaded: 1000, uploaded: 1500 }))).toBe(1.5);
    expect(ratioOf(torrent({ downloaded: 0, uploaded: 99 }))).toBeNull();
  });
});

describe('status', () => {
  it('uses the network unless stopped or failed', () => {
    for (const s of ['metadata', 'checking', 'connecting', 'downloading', 'seeding'] as const) {
      expect(isActive({ status: s })).toBe(true);
    }
    for (const s of ['paused', 'finished', 'error'] as const) expect(isActive({ status: s })).toBe(false);
  });
  it('has a main button that fits', () => {
    expect(primaryAction({ status: 'downloading' })).toBe('pause');
    expect(primaryAction({ status: 'seeding' })).toBe('pause');
    expect(primaryAction({ status: 'metadata' })).toBe('pause');
    expect(primaryAction({ status: 'paused' })).toBe('resume');
    expect(primaryAction({ status: 'finished' })).toBe('resume');
    expect(primaryAction({ status: 'error' })).toBe('retry');
  });
});

describe('filter and sort', () => {
  const list = [
    torrent({ id: 1, name: 'Zebra 10', size: 300, down_speed: 5 }),
    torrent({ id: 2, name: 'apple 2', size: null, down_speed: 50 }),
    torrent({ id: 3, name: 'Apple 10', size: 100, down_speed: 20, info_hash: 'cd'.repeat(20) }),
  ];
  it('filters by name or the start of the hash, ignoring case', () => {
    expect(filterTorrents(list, 'APPLE').map((t) => t.id)).toEqual([2, 3]);
    expect(filterTorrents(list, 'cdcd').map((t) => t.id)).toEqual([3]);
    expect(filterTorrents(list, '  ').length).toBe(3);
  });
  it('sorts names naturally', () => {
    expect(sortTorrents(list, 'name', 'asc', 'en').map((t) => t.id)).toEqual([2, 3, 1]);
  });
  it('puts unknown sizes last in both directions', () => {
    expect(sortTorrents(list, 'size', 'asc', 'en').map((t) => t.id)).toEqual([3, 1, 2]);
    expect(sortTorrents(list, 'size', 'desc', 'en').map((t) => t.id)).toEqual([1, 3, 2]);
  });
  it('sorts by speed and by date added (newest first when descending)', () => {
    expect(sortTorrents(list, 'down', 'desc', 'en').map((t) => t.id)).toEqual([2, 3, 1]);
    expect(sortTorrents(list, 'added', 'desc', 'en').map((t) => t.id)).toEqual([3, 2, 1]);
  });
  it('does not touch the list it was given', () => {
    const copy = list.map((t) => t.id);
    sortTorrents(list, 'name', 'desc', 'en');
    expect(list.map((t) => t.id)).toEqual(copy);
  });
});

describe('summary', () => {
  it('adds up the speeds of the torrents that run', () => {
    const s = summarizeTorrents([
      torrent({ status: 'downloading', down_speed: 100, up_speed: 10 }),
      torrent({ id: 2, status: 'seeding', up_speed: 40 }),
      torrent({ id: 3, status: 'paused', down_speed: 999 }),
      torrent({ id: 4, status: 'connecting' }),
    ]);
    expect(s).toEqual({ active: 3, downloading: 2, seeding: 1, down: 100, up: 50 });
  });
});

describe('live events', () => {
  it('adds a new torrent in front and replaces a known one', () => {
    const a = torrent({ id: 1, name: 'a' });
    const added = applyTorrentEvent([a], { type: 'torrent', torrent: torrent({ id: 2, name: 'b' }) });
    expect(added.map((t) => t.id)).toEqual([2, 1]);
    const replaced = applyTorrentEvent(added, { type: 'torrent', torrent: torrent({ id: 1, name: 'A!' }) });
    expect(replaced.map((t) => t.name)).toEqual(['b', 'A!']);
  });
  it('removes by id, and leaves the array alone when there is nothing to remove', () => {
    const list = [torrent({ id: 1 }), torrent({ id: 2 })];
    expect(applyTorrentEvent(list, { type: 'torrent_removed', id: 1 }).map((t) => t.id)).toEqual([2]);
    expect(applyTorrentEvent(list, { type: 'torrent_removed', id: 9 })).toBe(list);
  });
  it('ignores events of downloads', () => {
    const list = [torrent()];
    expect(applyTorrentEvent(list, { type: 'removed', id: 1 })).toBe(list);
  });
});

describe('speed history', () => {
  it('keeps the last window and drops what is older', () => {
    let h = pushSample([], { at: 0, down: 1, up: 0 });
    h = pushSample(h, { at: 2000, down: 2, up: 0 });
    h = pushSample(h, { at: HISTORY_MS + 5000, down: 3, up: 0 });
    expect(h.map((s) => s.down)).toEqual([2, 3]);
  });
  it('does not keep a sample that repeats the last one a moment later', () => {
    const h = pushSample([{ at: 0, down: 5, up: 1 }], { at: 500, down: 5, up: 1 });
    expect(h).toHaveLength(1);
    expect(pushSample(h, { at: 5000, down: 5, up: 1 })).toHaveLength(2);
  });
  it('draws a step chart that holds the last speed up to now', () => {
    const now = 10 * HISTORY_MS;
    const pts = chartPoints(
      [
        { at: now - HISTORY_MS / 2, down: 100, up: 0 },
        { at: now - HISTORY_MS / 4, down: 200, up: 50 },
      ],
      now,
    );
    expect(pts).toEqual([
      { x: 0.5, down: 100, up: 0 },
      { x: 0.75, down: 200, up: 50 },
      { x: 1, down: 200, up: 50 },
    ]);
  });
  it('starts the chart at the speed that was running before the window', () => {
    const now = 10 * HISTORY_MS;
    const pts = chartPoints([{ at: now - 2 * HISTORY_MS, down: 7, up: 1 }], now);
    expect(pts).toEqual([
      { x: 0, down: 7, up: 1 },
      { x: 1, down: 7, up: 1 },
    ]);
    expect(chartPoints([], now)).toEqual([]);
  });
  it('rounds the top of the axis to 1, 2 or 5 times a power of ten', () => {
    expect(niceMax(0)).toBe(1024);
    expect(niceMax(0.3)).toBe(0.5);
    expect(niceMax(1)).toBe(1);
    expect(niceMax(1.1)).toBe(2);
    expect(niceMax(3_400_000)).toBe(5_000_000);
    expect(niceMax(5_000_001)).toBe(10_000_000);
  });
});
