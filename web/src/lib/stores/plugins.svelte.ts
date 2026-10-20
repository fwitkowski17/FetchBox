// The plugin list: one REST fetch, kept fresh by the `plugin` and `plugin_removed` events of the
// shared event stream (the stream itself is opened by the downloads store).
import { api } from '../api/client';
import type { AppEvent, PluginView, SettingValue } from '../api/types';
import { i18n } from '../i18n';
import { applyPluginEvent, sortPlugins } from '../util/plugins';
import { toast } from './toast.svelte';

class PluginsStore {
  items = $state.raw<PluginView[]>([]);
  loaded = $state(false);
  /** The plugin system is on. Off with `--plugins off`, or while the server runs without a password. */
  available = $state(true);
  /** Why it is not available: `off` or `no_auth`. */
  reason = $state<string | null>(null);
  loadError = $state.raw<unknown>(null);

  get(id: string): PluginView | undefined {
    return this.items.find((p) => p.id === id);
  }

  async reload(): Promise<void> {
    try {
      const list = await api.listPlugins();
      this.items = sortPlugins(list.plugins);
      this.available = list.available;
      this.reason = list.reason ?? null;
      this.loaded = true;
      this.loadError = null;
    } catch (e) {
      this.loadError = e;
    }
  }

  /** Look at the plugins folder again; the list is replaced by the answer. */
  async rescan(): Promise<boolean> {
    try {
      const list = await api.rescanPlugins();
      this.items = sortPlugins(list.plugins);
      this.available = list.available;
      this.reason = list.reason ?? null;
      return true;
    } catch (e) {
      toast.error(i18n.errorText(e));
      return false;
    }
  }

  /** An event from the shared stream (`plugin`, `plugin_removed`). */
  received(ev: AppEvent): void {
    this.items = applyPluginEvent(this.items, ev);
  }

  /** A message from a plugin: shown as a toast with the plugin's name in front. */
  notice(ev: AppEvent): void {
    if (ev.type !== 'plugin_notice') return;
    const name = this.get(ev.plugin_id)?.name ?? ev.plugin_id;
    toast[ev.level](`${name}: ${ev.message}`);
  }

  /** Forget everything (sign-out). */
  clear(): void {
    this.items = [];
    this.loaded = false;
    this.loadError = null;
    this.available = true;
    this.reason = null;
  }

  // ---- commands: the stream brings the result, so the list is not patched here --------------

  setEnabled = (id: string, enabled: boolean) => this.#run(() => api.setPluginEnabled(id, enabled));
  approve = (id: string, hash: string) => this.#run(() => api.approvePlugin(id, hash));
  restart = (id: string) => this.#run(() => api.restartPlugin(id));
  fetchDependencies = (id: string, dependency?: string) =>
    this.#run(() => api.fetchPluginDependencies(id, dependency));
  remove = (id: string, keepData: boolean) => this.#run(() => api.removePlugin(id, keepData));

  /** Save settings. Throws: the card shows what was refused next to its fields. */
  async saveSettings(
    id: string,
    values: Record<string, SettingValue>,
    clear: string[] = [],
  ): Promise<PluginView> {
    return api.savePluginSettings(id, values, clear);
  }

  async #run(op: () => Promise<unknown>): Promise<boolean> {
    try {
      await op();
      return true;
    } catch (e) {
      toast.error(i18n.errorText(e));
      return false;
    }
  }
}

export const plugins = new PluginsStore();
