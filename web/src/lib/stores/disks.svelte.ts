// The disks downloads can be saved on (SPEC 6.14). Loaded when something needs them (the add
// dialog, Settings, the first-run step); the answer changes only when a disk is mounted or the
// default is changed, so there is no polling, but every user of it asks again when it opens.
import { api } from '../api/client';
import type { Disks } from '../api/types';

class DisksStore {
  /** `null` until the first answer, and when the server cannot tell. */
  value = $state.raw<Disks | null>(null);
  error = $state.raw<unknown>(null);

  async load(): Promise<Disks | null> {
    try {
      this.value = await api.getDisks();
      this.error = null;
    } catch (e) {
      this.error = e;
    }
    return this.value;
  }

  /** More than the download directory itself can be chosen. */
  get choosable(): boolean {
    return this.value?.choosable ?? false;
  }

  clear(): void {
    this.value = null;
    this.error = null;
  }
}

export const disks = new DisksStore();
