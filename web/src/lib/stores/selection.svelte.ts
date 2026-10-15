// Selecting several downloads to act on them together. `active` is the selection mode: the cards
// show checkboxes and the action bar appears. The selection only ever holds downloads that
// exist and are visible in the list.

class SelectionStore {
  active = $state(false);
  ids = $state.raw<ReadonlySet<number>>(new Set());

  get size(): number {
    return this.ids.size;
  }

  has(id: number): boolean {
    return this.ids.has(id);
  }

  enter(): void {
    this.active = true;
  }

  /** Leave the selection mode and forget the selection. */
  exit(): void {
    this.active = false;
    this.ids = new Set();
  }

  toggle(id: number): void {
    const next = new Set(this.ids);
    if (!next.delete(id)) next.add(id);
    this.ids = next;
  }

  /** Select exactly these. */
  setAll(ids: Iterable<number>): void {
    this.ids = new Set(ids);
  }

  clear(): void {
    this.ids = new Set();
  }

  /** Drop what is not in `allowed` any more (removed downloads, filtered-out ones). */
  keepOnly(allowed: ReadonlySet<number>): void {
    if ([...this.ids].every((id) => allowed.has(id))) return;
    this.ids = new Set([...this.ids].filter((id) => allowed.has(id)));
  }
}

export const selection = new SelectionStore();
