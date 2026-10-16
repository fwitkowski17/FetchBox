// The "new version" dialog: shown once after signing in when an update is known (unless the user
// asked to be reminded later or to skip that version), and on demand from Settings.
import type { VersionInfo } from '../api/types';
import { readJson, writeJson } from '../util/storage';

const SNOOZE_KEY = 'fetchbox.updateSnooze';
const SKIP_KEY = 'fetchbox.updateSkip';
/** "Remind me later" is a day. */
export const SNOOZE_MS = 24 * 60 * 60 * 1000;

interface Snooze {
  version: string;
  until: number;
}
const isSnooze = (v: unknown): v is Snooze =>
  typeof v === 'object' &&
  v !== null &&
  typeof (v as Snooze).version === 'string' &&
  typeof (v as Snooze).until === 'number';
const isString = (v: unknown): v is string => typeof v === 'string';

/** Should the dialog open by itself for this answer? Pure, so it is easy to test. */
export function shouldAnnounce(
  info: VersionInfo | null,
  snooze: Snooze | null,
  skipped: string | null,
  now: number,
): boolean {
  if (!info?.enabled || !info.update_available || !info.latest) return false;
  if (skipped === info.latest) return false;
  if (snooze && snooze.version === info.latest && snooze.until > now) return false;
  return true;
}

class UpdateNoticeStore {
  open = $state(false);
  /** `auto`: after signing in (Escape means "later"). `manual`: asked for in Settings (Escape just closes). */
  mode = $state<'auto' | 'manual'>('manual');
  #announced = false;

  /** Once per sign-in: open the dialog if there is news the user has not put off. */
  announce(info: VersionInfo | null): void {
    if (this.#announced) return;
    this.#announced = true;
    const snooze = readJson<Snooze | null>(SNOOZE_KEY, null, (v): v is Snooze | null => v === null || isSnooze(v));
    const skipped = readJson<string | null>(SKIP_KEY, null, (v): v is string | null => v === null || isString(v));
    if (shouldAnnounce(info, snooze, skipped, Date.now())) {
      this.mode = 'auto';
      this.open = true;
    }
  }

  /** Signing out starts over: the next sign-in may announce again. */
  reset(): void {
    this.#announced = false;
    this.open = false;
  }

  show(): void {
    this.mode = 'manual';
    this.open = true;
  }

  close(): void {
    this.open = false;
  }

  remindLater(version: string): void {
    writeJson(SNOOZE_KEY, { version, until: Date.now() + SNOOZE_MS } satisfies Snooze);
    this.open = false;
  }

  skip(version: string): void {
    writeJson(SKIP_KEY, version);
    this.open = false;
  }

  /** Escape, the backdrop: for the automatic dialog that is "remind me later". */
  dismiss(version: string | null): void {
    if (this.mode === 'auto' && version) this.remindLater(version);
    else this.close();
  }
}

export const updateNotice = new UpdateNoticeStore();
