// The settings form works with friendly values (KB/MB or Kbit/Mbit, "limit on/off"); the API with bytes/s and 0.
import type { Settings } from '../api/types';
import { joinSpeed, splitSpeed, type SpeedUnit } from './format';

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
  categorySubdirs: boolean;
  checkUpdates: boolean;
}

export function speedToDraft(bytesPerSecond: number): SpeedDraft {
  if (bytesPerSecond <= 0) return { limited: false, value: 1, unit: 'MB' };
  return { limited: true, ...splitSpeed(bytesPerSecond) };
}

export function speedFromDraft(d: SpeedDraft): number {
  return d.limited ? joinSpeed(d.value, d.unit) : 0;
}

export function toDraft(s: Settings): Draft {
  return {
    connections: s.connections,
    maxConcurrent: s.max_concurrent,
    perDownload: speedToDraft(s.speed_per_download),
    global: speedToDraft(s.speed_global),
    categorySubdirs: s.category_subdirs ?? false,
    // Older servers do not know the field; checks are on unless switched off.
    checkUpdates: s.check_updates ?? true,
  };
}

export function fromDraft(d: Draft): Settings {
  return {
    connections: d.connections,
    max_concurrent: d.maxConcurrent,
    speed_per_download: speedFromDraft(d.perDownload),
    speed_global: speedFromDraft(d.global),
    category_subdirs: d.categorySubdirs,
    check_updates: d.checkUpdates,
  };
}

export type Field = 'connections' | 'maxConcurrent' | 'perDownload' | 'global';

/** Which message to show for a field (`settings.error.<code>`); the UI supplies the wording. */
export type ErrorCode = 'connections' | 'concurrent' | 'speed';

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
  return errors;
}

export function sameSettings(a: Settings, b: Settings): boolean {
  return (
    a.connections === b.connections &&
    a.max_concurrent === b.max_concurrent &&
    a.speed_per_download === b.speed_per_download &&
    a.speed_global === b.speed_global &&
    (a.category_subdirs ?? false) === (b.category_subdirs ?? false) &&
    (a.check_updates ?? true) === (b.check_updates ?? true)
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
