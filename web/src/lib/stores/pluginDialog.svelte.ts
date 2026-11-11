// Which dialog of which plugin is open. A plugin declares its dialogs in its manifest; the host
// draws them (`PluginDialogHost`), so only the plugin's id and the dialog's id are kept here.

class PluginDialogStore {
  pluginId = $state<string | null>(null);
  dialogId = $state<string | null>(null);

  show(pluginId: string, dialogId: string): void {
    this.pluginId = pluginId;
    this.dialogId = dialogId;
  }

  hide(): void {
    this.pluginId = null;
    this.dialogId = null;
  }
}

export const pluginDialog = new PluginDialogStore();
