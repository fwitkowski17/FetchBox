// Installing a plugin: the files are unpacked and read first (nothing runs), the user looks at
// what the plugin asks for, and only then is it installed. The dialog lives once in the app shell.
import { api } from '../api/client';
import type { StagedPlugin } from '../api/types';
import { i18n, t } from '../i18n';
import { toast } from './toast.svelte';

class InstallPluginStore {
  staged = $state.raw<StagedPlugin | null>(null);
  /** Reading a file or installing is under way. */
  busy = $state(false);
  error = $state<string | null>(null);

  get open(): boolean {
    return this.staged !== null;
  }

  /** Unpack a ZIP chosen from disk and show what is in it. */
  async stageFile(file: File): Promise<void> {
    await this.#stage(async () => api.stagePlugin(await file.arrayBuffer()));
  }

  /** Fetch a plugin of the store and show what is in it. */
  async stageFromStore(id: string): Promise<void> {
    await this.#stage(() => api.stagePluginFromStore(id));
  }

  async install(): Promise<boolean> {
    const staged = this.staged;
    if (!staged || this.busy) return false;
    this.busy = true;
    this.error = null;
    try {
      const plugin = await api.installStaged(staged.token);
      this.staged = null;
      toast.success(t('ext.install.done', { name: plugin.name }));
      return true;
    } catch (e) {
      this.error = i18n.errorText(e);
      return false;
    } finally {
      this.busy = false;
    }
  }

  /** Close the dialog and throw the unpacked files away. */
  async cancel(): Promise<void> {
    const staged = this.staged;
    this.staged = null;
    this.error = null;
    if (!staged) return;
    try {
      await api.cancelStaged(staged.token);
    } catch {
      // The server forgets staged files by itself after a while.
    }
  }

  async #stage(op: () => Promise<StagedPlugin>): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    this.error = null;
    try {
      this.staged = await op();
    } catch (e) {
      toast.error(i18n.errorText(e));
    } finally {
      this.busy = false;
    }
  }
}

export const installPlugin = new InstallPluginStore();
