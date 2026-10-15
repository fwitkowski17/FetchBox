// The version card of Settings: what runs, and whether a newer release is known.
import { api } from '../api/client';
import type { VersionInfo } from '../api/types';

class VersionStore {
  /** `null` until the first answer. */
  info = $state.raw<VersionInfo | null>(null);
  /** A request is out (the first look at GitHub can take a few seconds). */
  busy = $state(false);
  /** The request itself failed (the server is not reachable); a failed look at GitHub is `info.check_failed`. */
  error = $state.raw<unknown>(null);

  #latest = 0;

  /** Ask for the kept answer, or, with `force`, look again now. */
  async load(force = false): Promise<void> {
    const mine = ++this.#latest;
    this.busy = true;
    try {
      const result = await (force ? api.checkVersion() : api.getVersion());
      if (mine !== this.#latest) return;
      this.info = result;
      this.error = null;
    } catch (e) {
      if (mine === this.#latest) this.error = e;
    } finally {
      if (mine === this.#latest) this.busy = false;
    }
  }
}

export const version = new VersionStore();
