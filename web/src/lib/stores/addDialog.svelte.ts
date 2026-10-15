// Whether the "add download" dialog is open. Anything can open it (menu, floating button, empty
// state, the old `#/add` link); the dialog itself lives once in the app shell.

class AddDialogStore {
  open = $state(false);

  show(): void {
    this.open = true;
  }

  hide(): void {
    this.open = false;
  }
}

export const addDialog = new AddDialogStore();
