// Free disk space, polled while the app is open. The server has no event for it: files grow
// continuously and a poll every few seconds is plenty for a gauge.
import { api } from '../api/client';
import type { DiskSpace } from '../api/types';

const POLL_MS = 10_000;

class DiskStore {
  /** `null` until the first answer, and when the server cannot tell (the ring is hidden then). */
  value = $state.raw<DiskSpace | null>(null);

  #timer: ReturnType<typeof setInterval> | null = null;

  async refresh(): Promise<void> {
    try {
      this.value = await api.getDisk();
    } catch {
      // Not worth a message: the gauge simply keeps its last value or stays hidden.
    }
  }

  /** Start polling (idempotent). A hidden tab does not poll; coming back refreshes at once. */
  start(): void {
    if (this.#timer) return;
    void this.refresh();
    this.#timer = setInterval(() => {
      if (!document.hidden) void this.refresh();
    }, POLL_MS);
    document.addEventListener('visibilitychange', this.#onVisible);
  }

  stop(): void {
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
    document.removeEventListener('visibilitychange', this.#onVisible);
    this.value = null;
  }

  #onVisible = (): void => {
    if (!document.hidden) void this.refresh();
  };
}

export const disk = new DiskStore();
