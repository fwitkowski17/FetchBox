// Which download the "delete" dialog is about (`null` = closed). The dialog lives once in the
// app shell; any card menu can open it.

class DeleteDialogStore {
  id = $state<number | null>(null);

  show(id: number): void {
    this.id = id;
  }

  hide(): void {
    this.id = null;
  }
}

export const deleteDialog = new DeleteDialogStore();
