// Download hours and start times (SPEC 6.3.2): the browser's side. The hours themselves are kept
// in minutes after midnight and in an IANA time zone; the server does the clock.

/** `390` -> `"06:30"`. */
export function minutesToTime(minutes: number): string {
  const m = Math.min(Math.max(Math.trunc(minutes), 0), 24 * 60 - 1);
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
}

/** `"06:30"` -> `390`; `null` for anything that is not a time of day. */
export function timeToMinutes(value: string): number | null {
  const m = /^([01]\d|2[0-3]):([0-5]\d)$/.exec(value.trim());
  return m?.[1] !== undefined && m[2] !== undefined ? Number(m[1]) * 60 + Number(m[2]) : null;
}

/** The time zone of this browser, which is where the user lives. */
export function browserZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

/** All the zone names the browser knows (empty in a browser that cannot list them). */
export function knownZones(): string[] {
  const intl = Intl as unknown as { supportedValuesOf?: (key: string) => string[] };
  try {
    return intl.supportedValuesOf?.('timeZone') ?? [];
  } catch {
    return [];
  }
}

/** Does the browser accept `name` as a time zone? (The server has the last word.) */
export function isZone(name: string): boolean {
  if (name.trim() === '') return false;
  try {
    new Intl.DateTimeFormat('en', { timeZone: name.trim() });
    return true;
  } catch {
    return false;
  }
}

/** The next time the local clock shows `time` (`"HH:MM"`), after `now`: today if it is still ahead,
 *  else tomorrow. `null` for text that is not a time. */
export function nextOccurrence(time: string, now: Date): Date | null {
  const minutes = timeToMinutes(time);
  if (minutes === null) return null;
  const at = new Date(now);
  at.setHours(Math.floor(minutes / 60), minutes % 60, 0, 0);
  if (at.getTime() <= now.getTime()) at.setDate(at.getDate() + 1);
  return at;
}

/** A moment (unix seconds) the way a person says it: the time of day when it is today, "tomorrow"
 *  and the time when it is tomorrow, otherwise the date and the time. */
export function formatWhen(seconds: number, locale: string, nowMs: number = Date.now()): string {
  const at = new Date(seconds * 1000);
  const time = at.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
  const midnight = (d: Date): number => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((midnight(at) - midnight(new Date(nowMs))) / 86_400_000);
  if (days === 0) return time;
  if (days === 1) {
    const word = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(1, 'day');
    return `${word} ${time}`;
  }
  return at.toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' });
}
