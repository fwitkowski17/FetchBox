// Whether the "add torrent" dialog is open, and a magnet link to start it with (for instance one
// pasted into the "add download" dialog). The dialog lives once in the app shell.

class AddTorrentDialogStore {
  open = $state(false);
  /** Text to put in the magnet field when the dialog opens. */
  magnet = $state('');

  show(magnet = ''): void {
    this.magnet = magnet;
    this.open = true;
  }

  hide(): void {
    this.open = false;
  }
}

export const addTorrentDialog = new AddTorrentDialogStore();
