import { describe, expect, it } from 'vitest';
import type { VersionInfo } from '../api/types';
import { shouldAnnounce } from './updateNotice.svelte';

const info = (over: Partial<VersionInfo> = {}): VersionInfo => ({
  current: '0.6.1',
  enabled: true,
  latest: '0.7.0',
  update_available: true,
  latest_prerelease: false,
  release_url: 'https://github.com/o/r/releases/tag/v0.7.0',
  checked_at: 1,
  check_failed: false,
  notes: [],
  ...over,
});
const NOW = 1_000_000;

describe('shouldAnnounce', () => {
  it('announces a new version', () => {
    expect(shouldAnnounce(info(), null, null, NOW)).toBe(true);
  });
  it('stays quiet without news, with checks off or before any answer', () => {
    expect(shouldAnnounce(null, null, null, NOW)).toBe(false);
    expect(shouldAnnounce(info({ update_available: false }), null, null, NOW)).toBe(false);
    expect(shouldAnnounce(info({ enabled: false }), null, null, NOW)).toBe(false);
    expect(shouldAnnounce(info({ latest: null }), null, null, NOW)).toBe(false);
  });
  it('respects "remind me later" until it runs out, for that version only', () => {
    expect(shouldAnnounce(info(), { version: '0.7.0', until: NOW + 1 }, null, NOW)).toBe(false);
    expect(shouldAnnounce(info(), { version: '0.7.0', until: NOW }, null, NOW)).toBe(true);
    expect(shouldAnnounce(info(), { version: '0.6.9', until: NOW + 1000 }, null, NOW)).toBe(true);
  });
  it('respects a skipped version but announces the next one', () => {
    expect(shouldAnnounce(info(), null, '0.7.0', NOW)).toBe(false);
    expect(shouldAnnounce(info({ latest: '0.8.0' }), null, '0.7.0', NOW)).toBe(true);
  });
});
