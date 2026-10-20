import { describe, expect, it } from 'vitest';
import type { PermissionSummary } from '../api/types';
import { permissionView } from './permissions';

function summary(over: Partial<PermissionSummary> = {}): PermissionSummary {
  return {
    api: [],
    download_hosts: [],
    events: [],
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
    expect(v.rows).toEqual([]);
    expect(v.widens).toBe(false);
  });

  it('lists only what is asked for, in a fixed order', () => {
    const v = permissionView(summary({ events: ['download.completed'], programs: ['yt-dlp'] }));
    expect(v.rows.map((r) => r.kind)).toEqual(['programs', 'events']);
    expect(v.rows[0]?.added).toEqual(['yt-dlp']);
    expect(v.none).toBe(false);
  });

  it('marks what an update adds', () => {
    const before = summary({ programs: ['yt-dlp'], download_hosts: ['github.com'] });
    const after = summary({
      programs: ['yt-dlp', 'ffmpeg'],
      download_hosts: ['github.com'],
      process: true,
    });
    const v = permissionView(after, before);
    expect(v.rows.find((r) => r.kind === 'programs')?.added).toEqual(['ffmpeg']);
    expect(v.rows.find((r) => r.kind === 'download_hosts')?.added).toEqual([]);
    expect(v.process).toEqual({ on: true, added: true });
    expect(v.widens).toBe(true);
  });

  it('an update that asks for less does not widen', () => {
    const v = permissionView(summary({ programs: ['a'] }), summary({ programs: ['a', 'b'], process: true }));
    expect(v.widens).toBe(false);
    expect(v.process).toEqual({ on: false, added: false });
  });
});
