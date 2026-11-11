<script lang="ts">
  // Draws the dialog a plugin declared. The fields are the plugin's, the frame is ours; a button
  // sends the typed values to the action it names and closes the dialog unless told otherwise.
  import { SvelteSet } from 'svelte/reactivity';
  import { api } from '../api/client';
  import type { PluginButton, PluginDialog, PluginView } from '../api/types';
  import { i18n, t } from '../i18n';
  import { pluginDialog } from '../stores/pluginDialog.svelte';
  import { plugins } from '../stores/plugins.svelte';
  import { toast } from '../stores/toast.svelte';
  import {
    actionOutcome,
    normalizeValues,
    tr,
    validateValues,
    valuesFromFields,
    type FieldError,
    type Values,
  } from '../util/pluginUi';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';
  import PluginFields from './PluginFields.svelte';

  const plugin = $derived(pluginDialog.pluginId ? plugins.get(pluginDialog.pluginId) : undefined);
  const dialog = $derived(plugin?.ui.dialogs.find((d) => d.id === pluginDialog.dialogId));

  // Keep what is drawn while the exit plays.
  let shown = $state.raw<{ plugin: PluginView; dialog: PluginDialog } | null>(null);
  let values = $state<Values>({});
  let errors = $state<Record<string, FieldError>>({});
  let busy = $state<string | null>(null);
  let failure = $state<string | null>(null);
  const noClearing = new SvelteSet<string>();

  // A different dialog starts from its defaults.
  let openKey = '';
  $effect(() => {
    if (plugin && dialog) {
      shown = { plugin, dialog };
      const key = `${plugin.id}/${dialog.id}`;
      if (key !== openKey) {
        openKey = key;
        values = valuesFromFields(dialog.fields ?? []);
        errors = {};
        failure = null;
      }
    } else {
      openKey = '';
    }
  });

  const L = (text: string) => (shown ? tr(shown.plugin.i18n, i18n.locale, text) : text);

  async function press(b: PluginButton): Promise<void> {
    if (!shown || busy) return;
    const { plugin: p, dialog: d } = shown;
    const closes = b.closes ?? b.action == null;
    if (b.action == null) {
      pluginDialog.hide();
      return;
    }
    errors = validateValues(d.fields ?? [], values);
    if (Object.keys(errors).length > 0) return;
    busy = b.id;
    failure = null;
    try {
      const outcome = actionOutcome(
        await api.runPluginAction(p.id, b.action, normalizeValues(d.fields ?? [], values)),
        t('ext.action.failed'),
      );
      if (outcome.notice) {
        if (outcome.close || closes) toast[outcome.notice.level](outcome.notice.message);
        else if (outcome.notice.level === 'error') failure = outcome.notice.message;
        else toast[outcome.notice.level](outcome.notice.message);
      }
      values = { ...values, ...outcome.set };
      if (outcome.openDialog) pluginDialog.show(p.id, outcome.openDialog);
      else if (outcome.close || (closes && outcome.ok)) pluginDialog.hide();
    } catch (e) {
      failure = i18n.errorText(e);
    } finally {
      busy = null;
    }
  }

  function submit(e: SubmitEvent): void {
    e.preventDefault();
    const primary = shown?.dialog.buttons?.find((b) => b.style === 'primary');
    if (primary) void press(primary);
  }
</script>

<Modal
  open={dialog !== undefined}
  title={shown ? L(shown.dialog.title) : ''}
  onclose={() => pluginDialog.hide()}
  size="lg"
>
  {#if shown}
    <form class="grid gap-4" onsubmit={submit} data-part="plugin-dialog" data-plugin={shown.plugin.id}>
      {#if shown.dialog.description}<p class="hint">{L(shown.dialog.description)}</p>{/if}
      <PluginFields
        fields={shown.dialog.fields ?? []}
        bind:values
        texts={shown.plugin.i18n}
        {errors}
        clearing={noClearing}
        idPrefix="plugin-dialog-{shown.plugin.id}-{shown.dialog.id}"
      />
      {#if failure}
        <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
          <Icon name="alert" size={18} />{failure}
        </p>
      {/if}
      <!-- Enter in a text field submits through the primary button. -->
      <button type="submit" hidden tabindex="-1" aria-hidden="true"></button>
    </form>
  {/if}
  {#snippet footer()}
    {#if shown}
      {#each shown.dialog.buttons ?? [] as b (b.id)}
        <button
          type="button"
          class={['btn', b.style === 'primary' && 'primary', b.style === 'danger' && 'danger']}
          disabled={busy !== null}
          onclick={() => void press(b)}
          data-button={b.id}
        >
          {L(b.label)}
        </button>
      {:else}
        <button type="button" class="btn" onclick={() => pluginDialog.hide()}>{t('common.close')}</button>
      {/each}
    {/if}
  {/snippet}
</Modal>
