// Which downloads the "delete" dialog is about (empty = closed). The dialog lives once in the app
// shell; a card menu opens it for one download, the selection bar for several.

class DeleteDialogStore {
  ids = $state.raw<readonly number[]>([]);

  get open(): boolean {
    return this.ids.length > 0;
  }

  show(...ids: number[]): void {
    this.ids = ids;
  }

  hide(): void {
    this.ids = [];
  }
}

export const deleteDialog = new DeleteDialogStore();
