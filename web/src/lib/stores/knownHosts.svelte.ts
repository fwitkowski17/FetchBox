// The SSH servers whose host key the user accepted (Settings lists them; SFTP downloads only go
// to these). Loaded when the card is opened: nothing changes them behind the page's back.
import { api } from '../api/client';
import type { KnownHost } from '../api/types';

class KnownHostsStore {
  /** `null` until the first answer. */
  list = $state.raw<KnownHost[] | null>(null);
  error = $state.raw<unknown>(null);

  async load(): Promise<void> {
    try {
      this.list = await api.getKnownHosts();
      this.error = null;
    } catch (e) {
      this.error = e;
    }
  }

  /** Forget a server; the list is reloaded either way, so it shows what the server has. */
  async forget(id: number): Promise<void> {
    try {
      await api.forgetHost(id);
    } finally {
      await this.load();
    }
  }
}

export const knownHosts = new KnownHostsStore();
