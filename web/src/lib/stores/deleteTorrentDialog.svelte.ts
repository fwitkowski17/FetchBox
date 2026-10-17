// Which torrent the "remove torrent" dialog is about (`null` = closed).

class DeleteTorrentDialogStore {
  id = $state<number | null>(null);

  get open(): boolean {
    return this.id !== null;
  }

  show(id: number): void {
    this.id = id;
  }

  hide(): void {
    this.id = null;
  }
}

export const deleteTorrentDialog = new DeleteTorrentDialogStore();
