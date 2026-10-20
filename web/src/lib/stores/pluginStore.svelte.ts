// The plugin store: what can be installed. Fetched when the store is opened (the server keeps the
// remote catalog cached, so asking again is cheap).
import { api } from '../api/client';
import type { StoreList } from '../api/types';
import { i18n } from '../i18n';
import { toast } from './toast.svelte';

class PluginStoreState {
  list = $state.raw<StoreList | null>(null);
  loading = $state(false);
  loadError = $state.raw<unknown>(null);
  query = $state('');

  async load(): Promise<void> {
    await this.#fetch(() => api.pluginStore());
  }

  /** Ask the remote catalog again now. */
  async refresh(): Promise<void> {
    await this.#fetch(() => api.refreshPluginStore());
  }

  clear(): void {
    this.list = null;
    this.loadError = null;
    this.query = '';
  }

  async #fetch(get: () => Promise<StoreList>): Promise<void> {
    this.loading = true;
    try {
      this.list = await get();
      this.loadError = null;
    } catch (e) {
      this.loadError = e;
      toast.error(i18n.errorText(e));
    } finally {
      this.loading = false;
    }
  }
}

export const pluginStore = new PluginStoreState();
