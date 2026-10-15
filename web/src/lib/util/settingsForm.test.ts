import { describe, expect, it } from 'vitest';
import type { Settings } from '../api/types';
import {
  MAX_CONNECTIONS,
  fromDraft,
  isUntouched,
  isValidSpeed,
  sameSettings,
  speedFromDraft,
  speedToDraft,
  toDraft,
  validate,
} from './settingsForm';

const base: Settings = {
  connections: 4,
  max_concurrent: 3,
  speed_per_download: 0,
  speed_global: 5 * 1024 * 1024,
  category_subdirs: true,
};

describe('settings form', () => {
  it('round-trips the server settings', () => {
    expect(fromDraft(toDraft(base))).toEqual(base);
    const odd: Settings = { ...base, speed_per_download: 300 * 1024, category_subdirs: false };
    expect(fromDraft(toDraft(odd))).toEqual(odd);
  });

  it('shows 0 as "no limit" and the rest in a friendly unit', () => {
    const d = toDraft(base);
    expect(d.perDownload.limited).toBe(false);
    expect(d.global).toEqual({ limited: true, value: 5, unit: 'MB' });
  });

  it('sends 0 when the limit is switched off, whatever the number says', () => {
    const d = toDraft(base);
    d.global = { limited: false, value: 7, unit: 'KB' };
    expect(fromDraft(d).speed_global).toBe(0);
  });

  it('treats a missing subdirs flag as off', () => {
    const { category_subdirs: _omit, ...legacy } = base;
    expect(toDraft(legacy).categorySubdirs).toBe(false);
  });

  it('validates counts and limits', () => {
    expect(validate(toDraft(base))).toEqual({});
    const d = toDraft(base);
    d.connections = 0;
    d.maxConcurrent = 2.5;
    d.perDownload = { limited: true, value: 0, unit: 'KB' };
    expect(validate(d)).toEqual({ connections: 'connections', maxConcurrent: 'concurrent', perDownload: 'speed' });
    d.connections = MAX_CONNECTIONS + 1;
    expect(validate(d).connections).toBeDefined();
    d.connections = MAX_CONNECTIONS;
    expect(validate(d).connections).toBeUndefined();
    d.global = { limited: true, value: Number.NaN, unit: 'MB' };
    expect(validate(d).global).toBeDefined();
  });

  it('knows what a usable speed limit is', () => {
    expect(isValidSpeed({ limited: false, value: 0, unit: 'KB' })).toBe(true);
    expect(isValidSpeed({ limited: true, value: 500, unit: 'KB' })).toBe(true);
    expect(isValidSpeed({ limited: true, value: 0, unit: 'KB' })).toBe(false);
    expect(isValidSpeed({ limited: true, value: -1, unit: 'MB' })).toBe(false);
    expect(isValidSpeed({ limited: true, value: Number.NaN, unit: 'MB' })).toBe(false);
  });

  it('turns a stored per-download limit into the dialog field and back', () => {
    expect(speedToDraft(0)).toEqual({ limited: false, value: 1, unit: 'MB' });
    const d = speedToDraft(750 * 1024);
    expect(d.limited).toBe(true);
    expect(speedFromDraft(d)).toBe(750 * 1024);
    expect(speedFromDraft({ ...d, limited: false })).toBe(0);
  });

  it('detects changes, treating missing and false subdirs as equal', () => {
    expect(sameSettings(base, { ...base })).toBe(true);
    expect(sameSettings(base, { ...base, connections: 5 })).toBe(false);
    expect(sameSettings(base, { ...base, speed_per_download: 1 })).toBe(false);
    const { category_subdirs: _omit, ...legacy } = base;
    expect(sameSettings(legacy, { ...base, category_subdirs: false })).toBe(true);
    expect(sameSettings(legacy, base)).toBe(false);
  });
});

describe('isUntouched', () => {
  it('is true before anything was loaded or edited', () => {
    expect(isUntouched(null, base)).toBe(true);
    expect(isUntouched(toDraft(base), null)).toBe(true);
    expect(isUntouched(toDraft(base), base)).toBe(true);
  });

  it('is false once the user changed something, even if the server changed too', () => {
    const edited = toDraft(base);
    edited.connections = 9;
    expect(isUntouched(edited, base)).toBe(false);
  });

  it('judges against the previous server value, so a change made elsewhere is not our edit', () => {
    const draft = toDraft(base); // what we showed
    const serverNow = { ...base, connections: 6 }; // someone else saved
    expect(sameSettings(fromDraft(draft), serverNow)).toBe(false); // looks "dirty" against the new value...
    expect(isUntouched(draft, base)).toBe(true); // ...but is untouched against the baseline
  });
});
