<script lang="ts">
  // The buttons a plugin declared. The browser sends only the action's id; what it runs is decided
  // by the manifest on the server.
  import { SvelteSet } from 'svelte/reactivity';
  import { api } from '../api/client';
  import type { ActionView, PluginView } from '../api/types';
  import { i18n, t } from '../i18n';
  import { confirmAction } from '../stores/confirm.svelte';
  import { pluginDialog } from '../stores/pluginDialog.svelte';
  import { toast } from '../stores/toast.svelte';
  import { actionOutcome, iconOrDefault, isSafeExternalUrl, tr } from '../util/pluginUi';
  import Icon, { ICON_NAMES } from './Icon.svelte';

  interface Props {
    plugin: PluginView;
    actions: readonly ActionView[];
    /** The plugin can run its actions (it is enabled and ready). */
    enabled?: boolean;
  }

  let { plugin, actions, enabled = true }: Props = $props();

  const running = new SvelteSet<string>();
  const L = (text: string) => tr(plugin.i18n, i18n.locale, text);

  async function run(a: ActionView): Promise<void> {
    if (a.kind === 'open_url') {
      if (isSafeExternalUrl(a.url)) window.open(a.url, '_blank', 'noopener,noreferrer');
      return;
    }
    if (a.confirm) {
      const ok = await confirmAction({
        title: L(a.label),
        message: L(a.confirm),
        confirmLabel: L(a.label),
      });
      if (!ok) return;
    }
    if (a.kind === 'open_dialog') {
      if (a.dialog) pluginDialog.show(plugin.id, a.dialog);
      return;
    }
    running.add(a.id);
    try {
      const outcome = actionOutcome(await api.runPluginAction(plugin.id, a.id), t('ext.action.failed'));
      if (outcome.notice) toast[outcome.notice.level](outcome.notice.message);
      if (outcome.openDialog) pluginDialog.show(plugin.id, outcome.openDialog);
    } catch (e) {
      toast.error(i18n.errorText(e));
    } finally {
      running.delete(a.id);
    }
  }
</script>

{#each actions as a (a.id)}
  <button
    type="button"
    class="btn"
    disabled={!enabled || running.has(a.id) || (a.kind === 'open_url' && !isSafeExternalUrl(a.url))}
    onclick={() => void run(a)}
    data-action={a.id}
  >
    {#if a.icon}<Icon name={iconOrDefault(a.icon, ICON_NAMES, 'other')} size={18} />{/if}
    {L(a.label)}
  </button>
{/each}
