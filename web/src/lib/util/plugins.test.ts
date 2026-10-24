import { describe, expect, it } from 'vitest';
import type { AppEvent, PluginView, StoreEntry } from '../api/types';
import {
  applyPluginEvent,
  attentionCount,
  cannotEnable,
  dependencyPercent,
  filterStore,
  isUsable,
  needsAttention,
  sortPlugins,
  stateTone,
} from './plugins';

function plugin(over: Partial<PluginView> = {}): PluginView {
  return {
    id: 'demo',
    name: 'Demo',
    version: '1.0.0',
    manifest_hash: 'ab'.repeat(32),
    description: '',
    author: '',
    source: 'zip',
    enabled: false,
    state: 'disabled',
    provides: { resolvers: [], hooks: 0, process: false },
    permissions: {
      groups: [],
      network: [],
      api: [],
      programs: [],
      download_hosts: [],
      webhook_hosts: [],
      events: [],
      process: false,
      runs_downloads: false,
    },
    settings: [],
    ui: { actions: [], dialogs: [] },
    dependencies: [],
    i18n: {},
    ...over,
  };
}

describe('applying plugin events', () => {
  it('adds a new plugin in name order and replaces a known one', () => {
    const b = plugin({ id: 'b', name: 'Bravo' });
    const a = plugin({ id: 'a', name: 'alpha' });
    const added = applyPluginEvent([b], { type: 'plugin', plugin: a });
    expect(added.map((p) => p.id)).toEqual(['a', 'b']);
    const changed = plugin({ id: 'a', name: 'alpha', state: 'running' });
    const replaced = applyPluginEvent(added, { type: 'plugin', plugin: changed });
    expect(replaced.map((p) => p.state)).toEqual(['running', 'disabled']);
    expect(replaced[1]).toBe(added[1]);
  });

  it('removes a plugin, and leaves the list alone for an unknown one', () => {
    const items = [plugin({ id: 'a' }), plugin({ id: 'b' })];
    const gone = applyPluginEvent(items, { type: 'plugin_removed', id: 'a' });
    expect(gone.map((p) => p.id)).toEqual(['b']);
    expect(applyPluginEvent(items, { type: 'plugin_removed', id: 'zzz' })).toBe(items);
  });

  it('ignores events that are not about plugins', () => {
    const items = [plugin()];
    const ev: AppEvent = { type: 'removed', id: 3 };
    expect(applyPluginEvent(items, ev)).toBe(items);
  });
});

describe('questions about a plugin', () => {
  it('sorts by name without caring for case, then by id', () => {
    const sorted = sortPlugins([
      plugin({ id: 'c', name: 'beta' }),
      plugin({ id: 'b', name: 'Alpha' }),
      plugin({ id: 'a', name: 'Alpha' }),
    ]);
    expect(sorted.map((p) => p.id)).toEqual(['a', 'b', 'c']);
  });

  it('knows what needs a look', () => {
    expect(needsAttention(plugin({ state: 'needs_review' }))).toBe(true);
    expect(needsAttention(plugin({ state: 'failed' }))).toBe(true);
    expect(needsAttention(plugin({ update_available: '2.0.0' }))).toBe(true);
    expect(needsAttention(plugin({ state: 'running' }))).toBe(false);
    expect(
      attentionCount([plugin({ state: 'failed' }), plugin({ state: 'ready' }), plugin()]),
    ).toBe(1);
  });

  it('says when a plugin is usable and when it cannot be switched on', () => {
    expect(isUsable(plugin({ state: 'ready' }))).toBe(true);
    expect(isUsable(plugin({ state: 'running' }))).toBe(true);
    expect(isUsable(plugin({ state: 'preparing' }))).toBe(false);
    expect(cannotEnable(plugin({ state: 'needs_review' }))).toBe('needs_review');
    expect(cannotEnable(plugin({ state: 'unsupported' }))).toBe('unsupported');
    expect(cannotEnable(plugin({ state: 'failed' }))).toBeNull();
    expect(cannotEnable(plugin())).toBeNull();
  });

  it('gives every state a tone', () => {
    expect(stateTone('running')).toBe('good');
    expect(stateTone('preparing')).toBe('busy');
    expect(stateTone('failed')).toBe('bad');
    expect(stateTone('needs_review')).toBe('warn');
    expect(stateTone('disabled')).toBe('muted');
  });

  it('shows the progress of a dependency only when the size is known', () => {
    expect(dependencyPercent(plugin())).toBeNull();
    expect(
      dependencyPercent(plugin({ progress: { dependency: 'x', downloaded: 50, total: null } })),
    ).toBeNull();
    expect(
      dependencyPercent(plugin({ progress: { dependency: 'x', downloaded: 50, total: 200 } })),
    ).toBe(25);
    expect(
      dependencyPercent(plugin({ progress: { dependency: 'x', downloaded: 500, total: 200 } })),
    ).toBe(100);
  });
});

describe('filterStore', () => {
  function entry(over: Partial<StoreEntry>): StoreEntry {
    return {
      id: 'x',
      name: 'X',
      version: '1.0.0',
      description: '',
      author: '',
      bundled: false,
      platforms: [],
      tags: [],
      unsupported: false,
      update_available: false,
      ...over,
    };
  }
  const entries = [
    entry({ id: 'metrics', name: 'Metrics', description: 'Sends numbers to a database', tags: ['monitoring'] }),
    entry({ id: 'yt-dlp', name: 'YouTube', description: 'Download videos', tags: ['video'] }),
    entry({ id: 'notify', name: 'Notify', description: 'Tell me about videos' }),
  ];

  it('keeps everything for an empty query', () => {
    expect(filterStore(entries, '  ').map((e) => e.id)).toEqual(['metrics', 'yt-dlp', 'notify']);
  });

  it('matches name, tags and description, in that order', () => {
    expect(filterStore(entries, 'video').map((e) => e.id)).toEqual(['yt-dlp', 'notify']);
    expect(filterStore(entries, 'YOUTUBE').map((e) => e.id)).toEqual(['yt-dlp']);
    expect(filterStore(entries, 'nothing')).toEqual([]);
  });
});
