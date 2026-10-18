// The settings form works with friendly values (KB/MB or Kbit/Mbit, "limit on/off"); the API with bytes/s and 0.
import type { Settings } from '../api/types';
import { joinSpeed, splitSpeed, type SpeedUnit } from './format';
import { browserZone, isZone, minutesToTime, timeToMinutes } from './schedule';

/** Mirrors the server's bound (`MAX_CONNECTIONS`). */
export const MAX_CONNECTIONS = 32;
/** UI-side sanity bound; the server only requires at least 1. */
export const MAX_CONCURRENT = 50;

export interface SpeedDraft {
  limited: boolean;
  value: number;
  unit: SpeedUnit;
}

export interface Draft {
  connections: number;
  maxConcurrent: number;
  perDownload: SpeedDraft;
  global: SpeedDraft;
  /** Speed limits of all torrents together: receiving and sending. */
  torrentDown: SpeedDraft;
  torrentUp: SpeedDraft;
  categorySubdirs: boolean;
  checkUpdates: boolean;
  /** Also announce pre-releases as updates. */
  checkPrereleases: boolean;
  /** Download only during set hours. */
  scheduleEnabled: boolean;
  /** `"HH:MM"`, as the time inputs have it. */
  scheduleFrom: string;
  scheduleTo: string;
  /** IANA time zone the hours are in. */
  scheduleTimezone: string;
  /** What running downloads do when the hours end. */
  scheduleOnEnd: 'pause' | 'finish';
  /** Let torrents run only during their own set hours (independent of the download hours). */
  torrentScheduleEnabled: boolean;
  torrentScheduleFrom: string;
  torrentScheduleTo: string;
  torrentScheduleTimezone: string;
  /** The disk new downloads are saved on by default; empty = the download directory itself. */
  defaultDisk: string;
}

export function speedToDraft(bytesPerSecond: number): SpeedDraft {
  if (bytesPerSecond <= 0) return { limited: false, value: 1, unit: 'MB' };
  return { limited: true, ...splitSpeed(bytesPerSecond) };
}

export function speedFromDraft(d: SpeedDraft): number {
  return d.limited ? joinSpeed(d.value, d.unit) : 0;
}

function scheduleZone(s: Settings): string {
  const zone = s.schedule_timezone ?? 'UTC';
  return !s.schedule_enabled && zone === 'UTC' ? browserZone() : zone;
}

function torrentScheduleZone(s: Settings): string {
  const zone = s.torrent_schedule_timezone ?? 'UTC';
  return !s.torrent_schedule_enabled && zone === 'UTC' ? browserZone() : zone;
}

export function toDraft(s: Settings): Draft {
  return {
    connections: s.connections,
    maxConcurrent: s.max_concurrent,
    perDownload: speedToDraft(s.speed_per_download),
    global: speedToDraft(s.speed_global),
    // Older servers do not know the torrent limits: none.
    torrentDown: speedToDraft(s.torrent_down_limit ?? 0),
    torrentUp: speedToDraft(s.torrent_up_limit ?? 0),
    categorySubdirs: s.category_subdirs ?? false,
    // Older servers do not know the field; checks are on unless switched off.
    checkUpdates: s.check_updates ?? true,
    checkPrereleases: s.check_prereleases ?? false,
    scheduleEnabled: s.schedule_enabled ?? false,
    scheduleFrom: minutesToTime(s.schedule_from ?? 0),
    scheduleTo: minutesToTime(s.schedule_to ?? 360),
    // The server's default is UTC. While the hours are off, offer the zone of this browser
    // instead, which is what a person who switches them on means.
    scheduleTimezone: scheduleZone(s),
    scheduleOnEnd: s.schedule_on_end ?? 'pause',
    // Older servers do not know the torrent hours: off.
    torrentScheduleEnabled: s.torrent_schedule_enabled ?? false,
    torrentScheduleFrom: minutesToTime(s.torrent_schedule_from ?? 0),
    torrentScheduleTo: minutesToTime(s.torrent_schedule_to ?? 360),
    torrentScheduleTimezone: torrentScheduleZone(s),
    // Older servers know one directory only.
    defaultDisk: s.default_disk ?? '',
  };
}

export function fromDraft(d: Draft): Settings {
  return {
    connections: d.connections,
    max_concurrent: d.maxConcurrent,
    speed_per_download: speedFromDraft(d.perDownload),
    speed_global: speedFromDraft(d.global),
    torrent_down_limit: speedFromDraft(d.torrentDown),
    torrent_up_limit: speedFromDraft(d.torrentUp),
    category_subdirs: d.categorySubdirs,
    check_updates: d.checkUpdates,
    check_prereleases: d.checkPrereleases,
    schedule_enabled: d.scheduleEnabled,
    schedule_from: timeToMinutes(d.scheduleFrom) ?? 0,
    schedule_to: timeToMinutes(d.scheduleTo) ?? 360,
    schedule_timezone: d.scheduleTimezone.trim(),
    schedule_on_end: d.scheduleOnEnd,
    torrent_schedule_enabled: d.torrentScheduleEnabled,
    torrent_schedule_from: timeToMinutes(d.torrentScheduleFrom) ?? 0,
    torrent_schedule_to: timeToMinutes(d.torrentScheduleTo) ?? 360,
    torrent_schedule_timezone: d.torrentScheduleTimezone.trim(),
    default_disk: d.defaultDisk,
  };
}

export type Field =
  | 'connections'
  | 'maxConcurrent'
  | 'perDownload'
  | 'global'
  | 'torrentDown'
  | 'torrentUp'
  | 'schedule'
  | 'timezone'
  | 'torrentSchedule'
  | 'torrentTimezone';

/** Which message to show for a field (`settings.error.<code>`); the UI supplies the wording. */
export type ErrorCode = 'connections' | 'concurrent' | 'speed' | 'schedule' | 'timezone';

export type Errors = Partial<Record<Field, ErrorCode>>;

/** A limit that is switched on needs a positive number. */
export function isValidSpeed(s: SpeedDraft): boolean {
  return !s.limited || (Number.isFinite(s.value) && s.value > 0);
}

export function validate(d: Draft): Errors {
  const errors: Errors = {};
  if (!Number.isInteger(d.connections) || d.connections < 1 || d.connections > MAX_CONNECTIONS) {
    errors.connections = 'connections';
  }
  if (!Number.isInteger(d.maxConcurrent) || d.maxConcurrent < 1 || d.maxConcurrent > MAX_CONCURRENT) {
    errors.maxConcurrent = 'concurrent';
  }
  if (!isValidSpeed(d.perDownload)) errors.perDownload = 'speed';
  if (!isValidSpeed(d.global)) errors.global = 'speed';
  if (!isValidSpeed(d.torrentDown)) errors.torrentDown = 'speed';
  if (!isValidSpeed(d.torrentUp)) errors.torrentUp = 'speed';
  if (d.scheduleEnabled) {
    const from = timeToMinutes(d.scheduleFrom);
    const to = timeToMinutes(d.scheduleTo);
    if (from === null || to === null || from === to) errors.schedule = 'schedule';
    if (!isZone(d.scheduleTimezone)) errors.timezone = 'timezone';
  }
  if (d.torrentScheduleEnabled) {
    const from = timeToMinutes(d.torrentScheduleFrom);
    const to = timeToMinutes(d.torrentScheduleTo);
    if (from === null || to === null || from === to) errors.torrentSchedule = 'schedule';
    if (!isZone(d.torrentScheduleTimezone)) errors.torrentTimezone = 'timezone';
  }
  return errors;
}

export function sameSettings(a: Settings, b: Settings): boolean {
  return (
    a.connections === b.connections &&
    a.max_concurrent === b.max_concurrent &&
    a.speed_per_download === b.speed_per_download &&
    a.speed_global === b.speed_global &&
    (a.torrent_down_limit ?? 0) === (b.torrent_down_limit ?? 0) &&
    (a.torrent_up_limit ?? 0) === (b.torrent_up_limit ?? 0) &&
    (a.category_subdirs ?? false) === (b.category_subdirs ?? false) &&
    (a.check_updates ?? true) === (b.check_updates ?? true) &&
    (a.check_prereleases ?? false) === (b.check_prereleases ?? false) &&
    (a.schedule_enabled ?? false) === (b.schedule_enabled ?? false) &&
    (a.schedule_from ?? 0) === (b.schedule_from ?? 0) &&
    (a.schedule_to ?? 360) === (b.schedule_to ?? 360) &&
    (a.schedule_timezone ?? 'UTC') === (b.schedule_timezone ?? 'UTC') &&
    (a.schedule_on_end ?? 'pause') === (b.schedule_on_end ?? 'pause') &&
    (a.torrent_schedule_enabled ?? false) === (b.torrent_schedule_enabled ?? false) &&
    (a.torrent_schedule_from ?? 0) === (b.torrent_schedule_from ?? 0) &&
    (a.torrent_schedule_to ?? 360) === (b.torrent_schedule_to ?? 360) &&
    (a.torrent_schedule_timezone ?? 'UTC') === (b.torrent_schedule_timezone ?? 'UTC') &&
    (a.default_disk ?? '') === (b.default_disk ?? '')
  );
}

/**
 * Has the user left the form as the server last showed it? Then it is safe to replace it with
 * newer server values. Compare against the *previous* server value (`baseline`), never the new
 * one: a change made elsewhere would otherwise look like an unsaved edit of ours.
 */
export function isUntouched(draft: Draft | null, baseline: Settings | null): boolean {
  return draft === null || baseline === null || !isDirty(draft, baseline);
}

/**
 * Does the form differ from what the server holds? The form shows speeds in a friendly unit and
 * rounds them (123 456 B/s is "120.56 KB/s", which is 123 453 B/s again), so the draft is
 * compared with the server value *as the form would show it*: an untouched form is never dirty,
 * and neither is one just saved with a value the form cannot show exactly (1.5 MB/s).
 */
export function isDirty(draft: Draft, server: Settings): boolean {
  return !sameSettings(fromDraft(draft), fromDraft(toDraft(server)));
}
