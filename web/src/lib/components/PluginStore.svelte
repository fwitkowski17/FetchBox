<script lang="ts">
  // The built-in store: ready-made extensions to install with one click.
  import { onMount } from 'svelte';
  import { i18n, t } from '../i18n';
  import { installPlugin } from '../stores/installPlugin.svelte';
  import { pluginStore } from '../stores/pluginStore.svelte';
  import { CARD } from '../util/classes';
  import { iconOrDefault, isSafeExternalUrl } from '../util/pluginUi';
  import Icon, { ICON_NAMES } from './Icon.svelte';

  onMount(() => void pluginStore.load());

  const entries = $derived(pluginStore.filtered);
</script>

<div class="grid gap-4" data-part="plugin-store">
  <div class="flex items-center gap-2">
    <div class="relative min-w-0 flex-1">
      <span class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-muted"><Icon name="search" size={18} /></span>
      <input
        class="input w-full pl-10"
        type="search"
        placeholder={t('ext.store.search')}
        aria-label={t('ext.store.search')}
        bind:value={pluginStore.query}
      />
    </div>
    <button type="button" class="btn" onclick={() => void pluginStore.refresh()} disabled={pluginStore.loading}>
      <Icon name="retry" size={18} />{t('ext.store.refresh')}
    </button>
  </div>

  {#if pluginStore.list?.error}
    <p class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" role="status">
      <Icon name="alert" size={18} />{t('ext.store.error', { error: pluginStore.list.error })}
    </p>
  {/if}

  {#if pluginStore.list === null}
    <p class="hint" role="status">{pluginStore.loadError ? i18n.errorText(pluginStore.loadError) : t('common.loading')}</p>
  {:else if entries.length === 0}
    <p class="hint" data-part="plugin-store-empty">{pluginStore.query ? t('ext.store.noMatch') : t('ext.store.empty')}</p>
  {:else}
    <ul class="grid gap-4 md:grid-cols-2">
      {#each entries as e (e.id)}
        <li class={CARD} data-part="store-entry" data-plugin={e.id}>
          <div class="flex items-start gap-3">
            <span class="mt-0.5 text-muted"><Icon name={iconOrDefault(e.icon, ICON_NAMES, 'puzzle')} size={24} /></span>
            <div class="min-w-0 flex-1">
              <h2 class="font-semibold tracking-tight [overflow-wrap:anywhere]">
                {e.name} <span class="text-sm font-normal text-muted">{e.version}</span>
              </h2>
              <p class="hint">{t('ext.by', { author: e.author })}{#if e.license} · {e.license}{/if}</p>
            </div>
          </div>
          <p class="[overflow-wrap:anywhere]">{e.description}</p>
          {#if e.tags.length > 0}
            <ul class="flex flex-wrap gap-1.5" aria-label={t('ext.store.tags')}>
              {#each e.tags as tag (tag)}<li class="rounded-md bg-surface-2 px-2 py-0.5 text-xs text-muted">{tag}</li>{/each}
            </ul>
          {/if}
          <div class="flex flex-wrap items-center gap-2">
            {#if isSafeExternalUrl(e.homepage)}
              <a class="text-sm" href={e.homepage} target="_blank" rel="noopener noreferrer">{t('ext.homepage')}</a>
            {/if}
            <span class="flex-1"></span>
            {#if e.unsupported}
              <span class="text-sm text-muted">{t('ext.store.unsupported')}</span>
            {:else if e.installed && !e.update_available}
              <span class="inline-flex items-center gap-1.5 text-sm text-success"><Icon name="check" size={16} />{t('ext.store.installed')}</span>
            {:else}
              <button
                type="button"
                class="btn primary"
                disabled={installPlugin.busy}
                onclick={() => void installPlugin.stageFromStore(e.id)}
                data-part="store-install"
              >
                {e.installed ? t('ext.update', { version: e.version }) : t('ext.store.install')}
              </button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>
