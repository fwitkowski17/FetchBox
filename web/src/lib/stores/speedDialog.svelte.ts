// Which download the "speed limit" dialog is about (`null` = closed). The dialog lives once in
// the app shell; a card menu opens it.

class SpeedDialogStore {
  id = $state<number | null>(null);

  show(id: number): void {
    this.id = id;
  }

  hide(): void {
    this.id = null;
  }
}

export const speedDialog = new SpeedDialogStore();
