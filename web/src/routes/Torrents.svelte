<script lang="ts">
  // The torrents: a list of their own, apart from the downloads. A click on a torrent opens its
  // details in a panel at the bottom of the page.
  import { fly } from 'svelte/transition';
  import Icon from '../lib/components/Icon.svelte';
  import TorrentPanel from '../lib/components/TorrentPanel.svelte';
  import TorrentRow from '../lib/components/TorrentRow.svelte';
  import { i18n, t } from '../lib/i18n';
  import { addTorrentDialog } from '../lib/stores/addTorrentDialog.svelte';
  import { torrents } from '../lib/stores/torrents.svelte';
  import { motion } from '../lib/util/motion';
  import {
    TORRENT_SORT_KEYS,
    filterTorrents,
    sortTorrents,
    summarizeTorrents,
    type TorrentSortKey,
  } from '../lib/util/torrents';

  const visible = $derived(
    sortTorrents(
      filterTorrents(torrents.items, torrents.query, torrents.group),
      torrents.sort,
      torrents.dir,
      i18n.locale,
    ),
  );
  const summary = $derived(summarizeTorrents(torrents.items));
  const sortLabel = $derived(t(`sort.${torrents.dir}`));

  // Leaving the page closes the details: the panel belongs to this page.
  $effect(() => () => torrents.closeDetails());
</script>

<header class="mb-4 flex flex-wrap items-end justify-between gap-x-4 gap-y-2">
  <div>
    <h1 class="text-2xl font-bold tracking-tight">{t('torrents.title')}</h1>
    <p class="mt-1 text-sm text-muted tabular-nums" data-part="summary">
      {#if summary.active > 0}
        <span class="inline-flex items-center gap-0.5 font-semibold text-accent"><Icon name="down" size={15} />{i18n.speed(summary.down)}</span>
        <span class="inline-flex items-center gap-0.5 font-semibold text-success"><Icon name="up" size={15} />{i18n.speed(summary.up)}</span>
        · {t('torrents.summary.active', { n: summary.active })}{summary.seeding > 0
          ? ` · ${t('torrents.summary.seeding', { n: summary.seeding })}`
          : ''}
      {:else}
        {t('torrents.summary.idle')}
      {/if}
    </p>
  </div>
  <button class="btn primary whitespace-nowrap max-lg:hidden" data-part="add-torrent-open" onclick={() => addTorrentDialog.show()}>
    <Icon name="plus" size={18} />{t('torrents.add')}
  </button>
</header>

{#if torrents.items.length > 0}
  <div class="mb-3 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
    <div class="relative">
      <span class="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-muted"><Icon name="search" size={18} /></span>
      <label class="sr-only" for="torrent-q">{t('torrents.search.label')}</label>
      <input
        id="torrent-q"
        class="input pl-10"
        type="search"
        placeholder={t('torrents.search.placeholder')}
        autocomplete="off"
        bind:value={torrents.query}
      />
    </div>
    <div class="flex gap-2">
      <label class="sr-only" for="torrent-sort">{t('torrents.sort.label')}</label>
      <select
        id="torrent-sort"
        class="select min-w-40 flex-1"
        value={torrents.sort}
        onchange={(e) => torrents.setSort(e.currentTarget.value as TorrentSortKey)}
      >
        {#each TORRENT_SORT_KEYS as key (key)}
          <option value={key}>{t(`torrents.sort.${key}`)}</option>
        {/each}
      </select>
      <button
        class="btn icon"
        onclick={() => torrents.toggleDir()}
        aria-label={t('downloads.sort.direction', { direction: sortLabel })}
        title={sortLabel}
      >
        <Icon name={torrents.dir === 'asc' ? 'up' : 'down'} />
      </button>
    </div>
  </div>
{/if}

{#if !torrents.loaded && !torrents.loadError}
  <p class="mt-10 text-center text-muted" role="status">{t('common.loading')}</p>
{:else if torrents.loadError && !torrents.loaded}
  <div class="mt-10 grid justify-items-center gap-3 text-center text-muted" role="alert">
    <p>{t('torrents.loadFailed', { error: i18n.errorText(torrents.loadError) })}</p>
    <button class="btn" onclick={() => torrents.reload()}>{t('common.tryAgain')}</button>
  </div>
{:else if torrents.items.length === 0}
  <div class="mt-12 grid justify-items-center gap-3 text-center text-muted" data-part="empty">
    <span class="grid size-16 place-items-center rounded-full bg-surface-2"><Icon name="magnet" size={28} /></span>
    <p>{t('torrents.empty')}</p>
    <button class="btn primary" onclick={() => addTorrentDialog.show()}>
      <Icon name="plus" size={18} />{t('torrents.empty.add')}
    </button>
  </div>
{:else if visible.length === 0}
  <p class="mt-12 text-center text-muted">{t('torrents.noMatch')}</p>
{:else}
  <ul class="mt-4 grid grid-cols-[minmax(0,1fr)] gap-2" aria-label={t('torrents.list.label')} data-part="torrent-list">
    {#each visible as torrent (torrent.id)}
      <li in:fly={{ y: 10, duration: motion(220) }}><TorrentRow {torrent} /></li>
    {/each}
  </ul>
{/if}

<TorrentPanel />
{#if torrents.selected}
  <!-- Room for the panel, so the last row is not hidden behind it. -->
  <div class="h-[min(24rem,55dvh)]" aria-hidden="true"></div>
{/if}
