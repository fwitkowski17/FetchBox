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
  perDownload: SpeedDraft;
  global: SpeedDraft;
  categorySubdirs: boolean;
}

function speedToDraft(bytesPerSecond: number): SpeedDraft {
  if (bytesPerSecond <= 0) return { limited: false, value: 1, unit: 'MB' };
  return { limited: true, ...splitSpeed(bytesPerSecond) };
}

function speedFromDraft(d: SpeedDraft): number {
  return d.limited ? joinSpeed(d.value, d.unit) : 0;
}

export function toDraft(s: Settings): Draft {
  return {
    connections: s.connections,
    maxConcurrent: s.max_concurrent,
    perDownload: speedToDraft(s.speed_per_download),
    global: speedToDraft(s.speed_global),
    categorySubdirs: s.category_subdirs ?? false,
  };
}

export function fromDraft(d: Draft): Settings {
  return {
    connections: d.connections,
    max_concurrent: d.maxConcurrent,
    speed_per_download: speedFromDraft(d.perDownload),
    speed_global: speedFromDraft(d.global),
    category_subdirs: d.categorySubdirs,
  };
}

export type Errors = Partial<Record<'connections' | 'maxConcurrent' | 'perDownload' | 'global', string>>;

export function validate(d: Draft): Errors {
  const errors: Errors = {};
  if (!Number.isInteger(d.connections) || d.connections < 1 || d.connections > MAX_CONNECTIONS) {
    errors.connections = `Use a whole number from 1 to ${MAX_CONNECTIONS}.`;
  }
  if (!Number.isInteger(d.maxConcurrent) || d.maxConcurrent < 1 || d.maxConcurrent > MAX_CONCURRENT) {
    errors.maxConcurrent = `Use a whole number from 1 to ${MAX_CONCURRENT}.`;
  }
  for (const key of ['perDownload', 'global'] as const) {
    const s = d[key];
    if (s.limited && !(Number.isFinite(s.value) && s.value > 0)) {
      errors[key] = 'Enter a speed above 0, or turn the limit off.';
    }
  }
  return errors;
}

export function sameSettings(a: Settings, b: Settings): boolean {
  return (
    a.connections === b.connections &&
    a.max_concurrent === b.max_concurrent &&
    a.speed_per_download === b.speed_per_download &&
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
