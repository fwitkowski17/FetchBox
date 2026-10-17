import { describe, expect, it } from 'vitest';
import type { Settings } from '../api/types';
import { browserZone } from './schedule';
import {
  MAX_CONNECTIONS,
  fromDraft,
  isDirty,
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
  check_updates: true,
  check_prereleases: false,
  schedule_enabled: false,
  schedule_from: 0,
  schedule_to: 360,
  schedule_timezone: 'Europe/Warsaw',
  schedule_on_end: 'pause',
  torrent_down_limit: 0,
  torrent_up_limit: 0,
  torrent_schedule_enabled: false,
  torrent_schedule_from: 0,
  torrent_schedule_to: 360,
  torrent_schedule_timezone: 'Europe/Warsaw',
};

describe('torrent limits', () => {
  it('are shown in a friendly unit and written back in bytes per second', () => {
    const s: Settings = { ...base, torrent_down_limit: 2 * 1024 * 1024, torrent_up_limit: 512 * 1024 };
    const d = toDraft(s);
    expect(d.torrentDown).toEqual({ limited: true, value: 2, unit: 'MB' });
    expect(d.torrentUp).toEqual({ limited: true, value: 512, unit: 'KB' });
    expect(fromDraft(d)).toEqual(s);
  });
  it('are none for a server that does not know them', () => {
    const { torrent_down_limit: _a, torrent_up_limit: _b, ...older } = base;
    expect(toDraft(older).torrentUp.limited).toBe(false);
    expect(isDirty(toDraft(older), older)).toBe(false);
  });
  it('count as unsaved edits and need a positive number when switched on', () => {
    const d = toDraft(base);
    d.torrentUp = { limited: true, value: 100, unit: 'KB' };
    expect(isDirty(d, base)).toBe(true);
    d.torrentUp = { limited: true, value: 0, unit: 'KB' };
    expect(validate(d).torrentUp).toBe('speed');
  });
});

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

  it('treats a missing update-check flag as on', () => {
    const { check_updates: _omit, ...legacy } = base;
    expect(toDraft(legacy).checkUpdates).toBe(true);
    expect(fromDraft({ ...toDraft(base), checkUpdates: false }).check_updates).toBe(false);
  });

  it('treats a missing pre-release flag as off and sends the choice back', () => {
    const { check_prereleases: _omit, ...legacy } = base;
    expect(toDraft(legacy).checkPrereleases).toBe(false);
    expect(fromDraft({ ...toDraft(base), checkPrereleases: true }).check_prereleases).toBe(true);
    expect(sameSettings(base, { ...base, check_prereleases: true })).toBe(false);
  });

  it('shows the download hours as times and keeps their zone', () => {
    const night: Settings = {
      ...base,
      schedule_enabled: true,
      schedule_from: 22 * 60,
      schedule_to: 6 * 60 + 30,
    };
    const d = toDraft(night);
    expect([d.scheduleEnabled, d.scheduleFrom, d.scheduleTo, d.scheduleTimezone]).toEqual([
      true,
      '22:00',
      '06:30',
      'Europe/Warsaw',
    ]);
    expect(fromDraft(d)).toEqual(night);
  });

  it('keeps the choice of what running downloads do when the hours end', () => {
    expect(toDraft(base).scheduleOnEnd).toBe('pause');
    const finish: Settings = { ...base, schedule_on_end: 'finish' };
    expect(toDraft(finish).scheduleOnEnd).toBe('finish');
    expect(fromDraft(toDraft(finish))).toEqual(finish);
    expect(isDirty(toDraft(base), finish)).toBe(true);
    const { schedule_on_end: _omit, ...older } = base;
    expect(toDraft(older).scheduleOnEnd).toBe('pause');
  });

  it('offers this browser\'s zone while the hours are off and the server still has the default', () => {
    const d = toDraft({ ...base, schedule_timezone: 'UTC' });
    expect(d.scheduleTimezone).toBe(browserZone());
    // That is not an edit: the form is compared with what it would show for the server's value.
    expect(isDirty(d, { ...base, schedule_timezone: 'UTC' })).toBe(false);
    // Once the hours are on, the zone is the user's and stays as stored.
    expect(toDraft({ ...base, schedule_enabled: true, schedule_timezone: 'UTC' }).scheduleTimezone).toBe('UTC');
  });

  it('checks the download hours only while they are on', () => {
    const d = toDraft(base);
    d.scheduleFrom = d.scheduleTo = '03:00';
    d.scheduleTimezone = 'Mars/Olympus';
    expect(validate(d)).toEqual({});
    d.scheduleEnabled = true;
    expect(validate(d)).toEqual({ schedule: 'schedule', timezone: 'timezone' });
    d.scheduleTo = '04:00';
    d.scheduleTimezone = 'Europe/Warsaw';
    expect(validate(d)).toEqual({});
    d.scheduleFrom = '';
    expect(validate(d).schedule).toBe('schedule');
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

  it('is dirty only when the user changed something, whatever the rounding of the form', () => {
    expect(isDirty(toDraft(base), base)).toBe(false);
    // Speeds the form cannot show exactly: still not dirty until the user edits something.
    for (const bytes of [123_456, 1_572_864, 99_999_999, 1]) {
      const odd: Settings = { ...base, speed_global: bytes, speed_per_download: bytes };
      expect(isDirty(toDraft(odd), odd), String(bytes)).toBe(false);
      expect(isUntouched(toDraft(odd), odd), String(bytes)).toBe(true);
    }
    const d = toDraft(base);
    d.connections += 1;
    expect(isDirty(d, base)).toBe(true);
    d.connections -= 1; // changing a field back removes the difference
    expect(isDirty(d, base)).toBe(false);
    d.global = { ...d.global, unit: 'Mbit' };
    expect(isDirty(d, base)).toBe(true);
    // A value typed as 1.5 MB/s is what the server then holds: saved, not dirty.
    const typed = toDraft(base);
    typed.perDownload = { limited: true, value: 1.5, unit: 'MB' };
    expect(isDirty(typed, { ...base, speed_per_download: fromDraft(typed).speed_per_download })).toBe(false);
  });
});


describe('torrent hours', () => {
  it('round-trip independently of the download hours', () => {
    const s: Settings = {
      ...base,
      schedule_enabled: true,
      schedule_from: 22 * 60,
      schedule_to: 6 * 60,
      torrent_schedule_enabled: true,
      torrent_schedule_from: 60,
      torrent_schedule_to: 7 * 60,
      torrent_schedule_timezone: 'Europe/Berlin',
    };
    const d = toDraft(s);
    expect(d.torrentScheduleFrom).toBe('01:00');
    expect(d.torrentScheduleTo).toBe('07:00');
    expect(d.scheduleFrom).toBe('22:00');
    expect(fromDraft(d)).toEqual(s);
  });

  it('are off for a server that does not know them, offering the browser zone', () => {
    const {
      torrent_schedule_enabled: _a,
      torrent_schedule_from: _b,
      torrent_schedule_to: _c,
      torrent_schedule_timezone: _d,
      ...older
    } = base;
    const d = toDraft(older);
    expect(d.torrentScheduleEnabled).toBe(false);
    expect(d.torrentScheduleTimezone).toBe(browserZone());
  });

  it('are only checked while switched on, and have their own error fields', () => {
    const d = toDraft(base);
    d.torrentScheduleFrom = '08:00';
    d.torrentScheduleTo = '08:00';
    expect(validate(d).torrentSchedule).toBeUndefined();
    d.torrentScheduleEnabled = true;
    expect(validate(d).torrentSchedule).toBe('schedule');
    expect(validate(d).schedule).toBeUndefined();
    d.torrentScheduleTo = '09:00';
    d.torrentScheduleTimezone = 'Nowhere/Land';
    expect(validate(d).torrentTimezone).toBe('timezone');
  });

  it('count as a change of the form', () => {
    const d = toDraft(base);
    expect(isDirty(d, base)).toBe(false);
    d.torrentScheduleEnabled = true;
    expect(isDirty(d, base)).toBe(true);
  });
});
