import { describe, expect, it } from 'vitest';
import type { PermissionSummary } from '../api/types';
import { groupReason, permissionView } from './permissions';

type Group = PermissionSummary['groups'][number];

function group(id: Group['id'], over: Partial<Group> = {}): Group {
  return { id, reason: null, reason_i18n: {}, ...over };
}

function summary(over: Partial<PermissionSummary> = {}): PermissionSummary {
  return {
    api: [],
    download_hosts: [],
    events: [],
    groups: [],
    network: [],
    process: false,
    programs: [],
    runs_downloads: false,
    webhook_hosts: [],
    ...over,
  };
}

describe('permissionView', () => {
  it('says when nothing is asked for', () => {
    const v = permissionView(summary());
    expect(v.none).toBe(true);
    expect(v.groups).toEqual([]);
    expect(v.widens).toBe(false);
  });

  it('shows the declared groups in a fixed order, with their lists', () => {
    const v = permissionView(
      summary({
        groups: [group('events'), group('programs', { reason: 'Runs yt-dlp.' })],
        events: ['download.completed'],
        programs: ['yt-dlp (dependency)'],
      }),
    );
    expect(v.groups.map((g) => g.id)).toEqual(['programs', 'events']);
    expect(v.groups[0]?.reason).toBe('Runs yt-dlp.');
    expect(v.groups[0]?.rows[0]?.added).toEqual(['yt-dlp (dependency)']);
    expect(v.groups[0]?.count).toBe(1);
    expect(v.none).toBe(false);
  });

  it('puts the three kinds of servers in one network group', () => {
    const v = permissionView(
      summary({
        groups: [group('network')],
        network: ['example.com'],
        download_hosts: ['github.com/o/r'],
        webhook_hosts: ['hooks.example'],
      }),
    );
    const g = v.groups[0];
    expect(g?.rows.map((r) => r.kind)).toEqual(['network', 'download_hosts', 'webhook_hosts']);
    expect(g?.count).toBe(3);
  });

  it('groups without a list are a yes', () => {
    const v = permissionView(
      summary({ groups: [group('background'), group('downloads')], process: true, runs_downloads: true }),
    );
    expect(v.groups.map((g) => [g.id, g.flag, g.rows.length])).toEqual([
      ['downloads', true, 0],
      ['background', true, 0],
    ]);
  });

  it('still shows what the manifest did not declare, and says so', () => {
    const v = permissionView(summary({ programs: ['x'] }));
    expect(v.groups).toHaveLength(1);
    expect(v.groups[0]?.declared).toBe(false);
  });

  it('marks what an update adds', () => {
    const before = summary({
      groups: [group('programs'), group('network')],
      programs: ['yt-dlp'],
      download_hosts: ['github.com'],
    });
    const after = summary({
      groups: [group('programs'), group('network'), group('background')],
      programs: ['yt-dlp', 'ffmpeg'],
      download_hosts: ['github.com'],
      process: true,
    });
    const v = permissionView(after, before);
    const by = (id: string) => v.groups.find((g) => g.id === id);
    expect(by('programs')?.rows[0]?.added).toEqual(['ffmpeg']);
    expect(by('programs')?.added).toBe(true);
    expect(by('network')?.added).toBe(false);
    expect(by('background')?.added).toBe(true);
    expect(v.widens).toBe(true);
  });

  it('an update that asks for less does not widen', () => {
    const v = permissionView(
      summary({ groups: [group('programs')], programs: ['a'] }),
      summary({ groups: [group('programs'), group('background')], programs: ['a', 'b'], process: true }),
    );
    expect(v.widens).toBe(false);
    expect(v.groups.every((g) => !g.added)).toBe(true);
  });
});

describe('groupReason', () => {
  const g = permissionView(
    summary({
      groups: [group('network', { reason: 'Talks.', reason_i18n: { pl: 'Rozmawia.' } })],
      network: ['a'],
    }),
  ).groups[0]!;

  it('uses the language of the interface, then the bare language, then English', () => {
    expect(groupReason(g, 'pl')).toBe('Rozmawia.');
    expect(groupReason(g, 'pl-PL')).toBe('Rozmawia.');
    expect(groupReason(g, 'de')).toBe('Talks.');
  });

  it('is null when the author gave no reason', () => {
    const none = permissionView(summary({ groups: [group('api')], api: ['downloads.read'] })).groups[0]!;
    expect(groupReason(none, 'pl')).toBeNull();
  });
});
