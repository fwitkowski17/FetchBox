import { api } from '../api/client';
import type { Settings } from '../api/types';

class SettingsStore {
  /** `null` until the first load. */
  value = $state.raw<Settings | null>(null);
  error = $state<string | null>(null);

  async load(): Promise<void> {
    try {
      this.value = await api.getSettings();
      this.error = null;
    } catch (e) {
      this.error = e instanceof Error ? e.message : 'Cannot load settings';
    }
  }

  /** Throws `ApiError` on validation failure so the form can show the message. */
  async save(next: Settings): Promise<void> {
    this.value = await api.putSettings(next);
  }

  /** Pushed by the server when another client changed the settings. */
  received(s: Settings): void {
    this.value = s;
  }

  clear(): void {
    this.value = null;
  }
}

export const settings = new SettingsStore();
