import { describe, expect, it } from 'vitest';
import type { DependencyNeed, PermissionSummary, StagedPlugin } from '../api/types';
import { dependencyReview } from './dependencies';

function need(id: string, over: Partial<DependencyNeed> = {}): DependencyNeed {
  return {
    id,
    label: id,
    optional: false,
    source: 'github',
    hosts: ['github.com/o/r'],
    size: null,
    ...over,
  };
}

function summary(dependencies: DependencyNeed[]): PermissionSummary {
  return {
    api: [],
    dependencies,
    events: [],
    groups: [],
    network: [],
    process: false,
    programs: [],
    runs_downloads: false,
    webhook_hosts: [],
  };
}

function staged(
  dependencies: DependencyNeed[],
  plan: StagedPlugin['dependency_plan'],
  replaces: StagedPlugin['replaces'] = null,
): StagedPlugin {
  return {
    token: 't',
    id: 'demo',
    name: 'Demo',
    version: '2.0.0',
    description: '',
    author: 'A',
    source: 'store',
    permissions: summary(dependencies),
    dependency_plan: plan,
    replaces,
  } as StagedPlugin;
}

describe('dependencyReview', () => {
  it('has nothing to confirm without dependencies', () => {
    const r = dependencyReview(staged([], []));
    expect(r.rows).toEqual([]);
    expect(r.confirm).toBe(false);
    expect(r.bytes).toBe(0);
  });

  it('asks for a yes only when something is going to be downloaded', () => {
    const deps = [need('yt-dlp'), need('ffmpeg', { optional: true, source: 'shared', size: 30_000_000 })];
    const r = dependencyReview(
      staged(deps, [
        { id: 'yt-dlp', plan: 'download' },
        { id: 'ffmpeg', plan: 'on_request' },
      ]),
    );
    expect(r.confirm).toBe(true);
    expect(r.downloads.map((d) => d.need.id)).toEqual(['yt-dlp']);

    // Optional only, or already on the machine: nothing to agree to.
    const none = dependencyReview(
      staged(deps, [
        { id: 'yt-dlp', plan: 'present' },
        { id: 'ffmpeg', plan: 'on_request' },
      ]),
    );
    expect(none.confirm).toBe(false);
    expect(none.rows.map((r2) => r2.plan)).toEqual(['present', 'on_request']);
  });

  it('adds up the sizes it knows and counts the ones it does not', () => {
    const r = dependencyReview(
      staged(
        [need('a', { size: 100 }), need('b', { size: 50 }), need('c'), need('d', { size: 7, optional: true })],
        [
          { id: 'a', plan: 'download' },
          { id: 'b', plan: 'download' },
          { id: 'c', plan: 'download' },
          { id: 'd', plan: 'on_request' },
        ],
      ),
    );
    expect(r.bytes).toBe(150);
    expect(r.unknownSizes).toBe(1);
  });

  it('marks what an update adds and says whether the download starts at once', () => {
    const previous = summary([need('old')]);
    const dependencies = [need('old'), need('fresh')];
    const plan = [
      { id: 'old', plan: 'present' as const },
      { id: 'fresh', plan: 'download' as const },
    ];
    const on = dependencyReview(
      staged(dependencies, plan, { version: '1.0.0', permissions: previous, enabled: true }),
    );
    expect(on.rows.map((r) => r.added)).toEqual([false, true]);
    expect(on.when).toBe('now');

    const off = dependencyReview(
      staged(dependencies, plan, { version: '1.0.0', permissions: previous, enabled: false }),
    );
    expect(off.when).toBe('on_enable');

    // A first install marks nothing as new.
    const first = dependencyReview(staged(dependencies, plan));
    expect(first.rows.every((r) => !r.added)).toBe(true);
    expect(first.when).toBe('on_enable');
  });

  it('falls back to the manifest when the server sent no plan', () => {
    const r = dependencyReview(staged([need('a'), need('b', { optional: true })], []));
    expect(r.rows.map((x) => x.plan)).toEqual(['download', 'on_request']);
  });
});
