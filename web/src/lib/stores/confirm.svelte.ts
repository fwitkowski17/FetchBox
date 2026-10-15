// One shared confirmation dialog, used as `if (await confirmAction({...})) { ... }`.

export interface ConfirmOptions {
  title: string;
  message: string;
  confirmLabel: string;
  /** Style the confirm button as destructive. */
  danger?: boolean;
}

interface Pending extends ConfirmOptions {
  resolve: (ok: boolean) => void;
}

class ConfirmStore {
  current = $state.raw<Pending | null>(null);

  ask(options: ConfirmOptions): Promise<boolean> {
    // A second question replaces the first, which counts as "no".
    this.current?.resolve(false);
    return new Promise((resolve) => {
      this.current = { ...options, resolve };
    });
  }

  answer(ok: boolean): void {
    const pending = this.current;
    this.current = null;
    pending?.resolve(ok);
  }
}

export const confirmStore = new ConfirmStore();
export const confirmAction = (options: ConfirmOptions): Promise<boolean> => confirmStore.ask(options);
