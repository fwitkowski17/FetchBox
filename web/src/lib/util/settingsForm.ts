// The settings form works with friendly values (KB/MB, "limit on/off"); the API with bytes/s and 0.
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
  global: SpeedDraft;
  categorySubdirs: boolean;
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
    global: speedToDraft(s.speed_global),
    categorySubdirs: s.category_subdirs ?? false,
  };
}

export function fromDraft(d: Draft): Settings {
  return {
    connections: d.connections,
    max_concurrent: d.maxConcurrent,
    speed_global: speedFromDraft(d.global),
    category_subdirs: d.categorySubdirs,
  };
}

export type Field = 'connections' | 'maxConcurrent' | 'global';

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
  if (!isValidSpeed(d.global)) errors.global = 'speed';
  return errors;
}

export function sameSettings(a: Settings, b: Settings): boolean {
  return (
    a.connections === b.connections &&
    a.max_concurrent === b.max_concurrent &&
    a.speed_global === b.speed_global &&
    (a.category_subdirs ?? false) === (b.category_subdirs ?? false)
  );
}

/**
 * Has the user left the form as the server last showed it? Then it is safe to replace it with
 * newer server values. Compare against the *previous* server value (`baseline`), never the new
 * one: a change made elsewhere would otherwise look like an unsaved edit of ours.
 */
export function isUntouched(draft: Draft | null, baseline: Settings | null): boolean {
  return draft === null || baseline === null || sameSettings(fromDraft(draft), baseline);
}
