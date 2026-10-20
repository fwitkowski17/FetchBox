<script lang="ts">
  // The settings of one plugin with a save button of its own: a plugin's settings are saved by the
  // plugin's endpoint, apart from the global settings form. With `sections` (the plugin's own tab)
  // the fields and actions are grouped as the plugin declared; without, all settings in one block.
  import { untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import type { PluginField, PluginSection, PluginView } from '../api/types';
  import { i18n, t } from '../i18n';
  import { plugins } from '../stores/plugins.svelte';
  import { toast } from '../stores/toast.svelte';
  import {
    hasChange,
    isInput,
    settingsChange,
    tr,
    validateValues,
    valuesFromViews,
    type Values,
  } from '../util/pluginUi';
  import Icon from './Icon.svelte';
  import PluginActions from './PluginActions.svelte';
  import PluginFields from './PluginFields.svelte';

  interface Props {
    plugin: PluginView;
    sections?: readonly PluginSection[];
  }

  let { plugin, sections }: Props = $props();

  let values = $state<Values>({});
  const clearing = new SvelteSet<string>();
  let saving = $state(false);
  let failure = $state<string | null>(null);
  let showErrors = $state(false);

  const fields = $derived(plugin.settings.map((v) => v.field));
  const stored = $derived(new Set(plugin.settings.filter((v) => v.has_value).map((v) => v.field.key)));
  const L = (text: string) => tr(plugin.i18n, i18n.locale, text);

  // The form follows the server until the user has edited it.
  const change = $derived(settingsChange(plugin.settings, values, clearing));
  const dirty = $derived(hasChange(change));
  const errors = $derived(validateValues(fields, values, stored));
  const usable = $derived(plugin.state === 'ready' || plugin.state === 'running');

  const serverKey = $derived(
    JSON.stringify([plugin.id, plugin.settings.map((v) => [v.value, v.has_value])]),
  );
  let syncedKey = '';
  let syncedId = '';

  function load(): void {
    values = valuesFromViews(plugin.settings);
    clearing.clear();
    syncedKey = serverKey;
    syncedId = plugin.id;
  }

  $effect(() => {
    const key = serverKey;
    untrack(() => {
      if (key !== syncedKey && (plugin.id !== syncedId || !dirty)) load();
    });
  });

  function reset(): void {
    load();
    showErrors = false;
    failure = null;
  }

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    showErrors = true;
    if (!dirty || saving || Object.keys(errors).length > 0) return;
    saving = true;
    failure = null;
    try {
      await plugins.saveSettings(plugin.id, change.values, change.clear);
      toast.success(t('ext.settings.saved'));
      showErrors = false;
    } catch (err) {
      failure = i18n.errorText(err);
    } finally {
      saving = false;
    }
  }

  function fieldsOf(keys: readonly string[] | undefined): PluginField[] {
    if (!keys) return [];
    return keys.flatMap((k) => fields.filter((f) => f.key === k));
  }

  const groups = $derived(
    sections && sections.length > 0
      ? sections.map((s) => ({
          title: s.title ?? '',
          fields: fieldsOf(s.fields),
          actions: (s.actions ?? []).flatMap((id) => plugin.ui.actions.filter((a) => a.id === id)),
        }))
      : [
          {
            title: '',
            fields,
            actions: plugin.ui.actions.filter((a) => a.placement === 'settings'),
          },
        ],
  );
</script>

<form class="grid gap-4" onsubmit={save} data-part="plugin-settings" data-plugin={plugin.id}>
  {#if !usable}
    <p class="hint">{t(plugin.state === 'disabled' ? 'ext.settings.disabled' : 'ext.settings.notReady')}</p>
  {/if}
  {#each groups as g, i (i)}
    <div class="grid gap-4">
      {#if g.title}<h3 class="font-semibold">{L(g.title)}</h3>{/if}
      <PluginFields
        fields={g.fields}
        bind:values
        texts={plugin.i18n}
        errors={showErrors ? errors : {}}
        {stored}
        {clearing}
        idPrefix="plugin-{plugin.id}"
      />
      {#if g.actions.length > 0}
        <div class="flex flex-wrap gap-2">
          <PluginActions {plugin} actions={g.actions} enabled={usable} />
        </div>
      {/if}
    </div>
  {/each}
  {#if fields.some(isInput)}
    {#if failure}
      <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
        <Icon name="alert" size={18} />{failure}
      </p>
    {/if}
    <div class="flex items-center justify-end gap-2">
      {#if dirty}<span class="hint flex-1">{t('settings.unsaved')}</span>{/if}
      <button type="button" class="btn" onclick={reset} disabled={!dirty || saving}>{t('settings.discard')}</button>
      <button type="submit" class="btn primary" disabled={!dirty || saving} data-part="plugin-settings-save">
        {saving ? t('settings.saving') : t('settings.save')}
      </button>
    </div>
  {/if}
</form>
