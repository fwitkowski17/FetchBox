<script lang="ts">
  // The Extensions page: the plugins that are installed, and the store with ready-made ones.
  // Installing is also possible from a ZIP file on disk (it is unpacked and shown for review
  // first); a folder copied into the plugins folder is picked up by "Rescan".
  import { onMount } from 'svelte';
  import { t } from '../lib/i18n';
  import Icon from '../lib/components/Icon.svelte';
  import PluginCard from '../lib/components/PluginCard.svelte';
  import PluginStore from '../lib/components/PluginStore.svelte';
  import Tabs from '../lib/components/Tabs.svelte';
  import { installPlugin } from '../lib/stores/installPlugin.svelte';
  import { plugins } from '../lib/stores/plugins.svelte';
  import { routeTail } from '../lib/stores/route';
  import { CARD } from '../lib/util/classes';
  import { attentionCount } from '../lib/util/plugins';

  const tab = $derived($routeTail[0] === 'store' ? 'store' : 'installed');
  const attention = $derived(attentionCount(plugins.items));
  let fileInput: HTMLInputElement | undefined = $state();
  let rescanning = $state(false);

  onMount(() => {
    if (!plugins.loaded) void plugins.reload();
  });

  function select(id: string): void {
    location.hash = id === 'store' ? '#/extensions/store' : '#/extensions';
  }

  async function chosen(e: Event & { currentTarget: HTMLInputElement }): Promise<void> {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = '';
    if (file) await installPlugin.stageFile(file);
  }

  async function rescan(): Promise<void> {
    rescanning = true;
    await plugins.rescan();
    rescanning = false;
  }
</script>

<div class="page-narrow grid gap-4">
  <header class="flex flex-wrap items-center gap-2">
    <h1 class="flex-1 text-2xl font-bold tracking-tight">{t('ext.title')}</h1>
    {#if plugins.available}
      <button type="button" class="btn" onclick={() => void rescan()} disabled={rescanning} data-part="plugin-rescan">
        <Icon name="retry" size={18} />{t('ext.rescan')}
      </button>
      <button
        type="button"
        class="btn primary"
        onclick={() => fileInput?.click()}
        disabled={installPlugin.busy}
        data-part="plugin-from-file"
      >
        <Icon name="plus" size={18} />{t('ext.fromFile')}
      </button>
      <input
        bind:this={fileInput}
        type="file"
        accept=".zip,application/zip"
        class="hidden"
        onchange={chosen}
        aria-label={t('ext.fromFile')}
      />
    {/if}
  </header>

  {#if !plugins.available}
    <section class={CARD} role="status" data-part="plugins-unavailable">
      <h2 class="font-semibold tracking-tight">{t('ext.unavailable.title')}</h2>
      <p>{t(plugins.reason === 'no_auth' ? 'ext.unavailable.noAuth' : 'ext.unavailable.off')}</p>
    </section>
  {:else}
    <Tabs
      tabs={[
        { id: 'installed', label: t('ext.tab.installed'), icon: 'puzzle', dot: attention > 0 },
        { id: 'store', label: t('ext.tab.store'), icon: 'down' },
      ]}
      selected={tab}
      onselect={select}
      label={t('ext.title')}
      idPrefix="ext"
    />
    <div id="ext-tabpanel" role="tabpanel" aria-labelledby="ext-tab-{tab}" tabindex="0" class="grid gap-4">
      {#if tab === 'store'}
        <PluginStore />
      {:else if plugins.loadError && !plugins.loaded}
        <p class="text-warning" role="alert">{t('ext.loadFailed')}</p>
        <div><button type="button" class="btn" onclick={() => void plugins.reload()}>{t('common.tryAgain')}</button></div>
      {:else if !plugins.loaded}
        <p class="hint" role="status">{t('common.loading')}</p>
      {:else if plugins.items.length === 0}
        <section class={CARD} data-part="plugins-empty">
          <h2 class="font-semibold tracking-tight">{t('ext.empty.title')}</h2>
          <p class="hint">{t('ext.empty.hint')}</p>
          <div><a class="btn primary" href="#/extensions/store">{t('ext.empty.openStore')}</a></div>
        </section>
      {:else}
        {#each plugins.items as p (p.id)}
          <PluginCard plugin={p} />
        {/each}
      {/if}
    </div>
  {/if}
</div>
