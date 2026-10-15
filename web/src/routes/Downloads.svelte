<script lang="ts">
  import DownloadCard from '../lib/components/DownloadCard.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { i18n, t } from '../lib/i18n';
  import { downloads } from '../lib/stores/downloads.svelte';
  import { prefs } from '../lib/stores/prefs.svelte';
  import { toast } from '../lib/stores/toast.svelte';
  import {
    SORT_OPTIONS,
    filterDownloads,
    sortDownloads,
    summarize,
    type SortKey,
  } from '../lib/util/downloads';

  const filter = $derived({ group: prefs.group, category: prefs.category, query: prefs.query });
  const visible = $derived(
    sortDownloads(filterDownloads(downloads.items, filter), prefs.sort, i18n.locale),
  );
  const summary = $derived(summarize(downloads.items));
  const finished = $derived(
    downloads.items.filter((d) => d.status === 'completed' || d.status === 'failed' || d.status === 'cancelled').length,
  );
  const filtering = $derived(prefs.group !== 'all' || prefs.category !== 'all' || prefs.query.trim() !== '');

  const sortLabel = $derived(t(`sort.${prefs.sort.dir}`));

  async function clearFinished(): Promise<void> {
    const n = finished;
    await downloads.clearFinished();
    toast.success(t('downloads.cleared', { n }));
  }

  function resetFilters(): void {
    prefs.setGroup('all');
    prefs.setCategory('all');
    prefs.query = '';
  }
</script>

<header class="head">
  <div>
    <h1>{t('downloads.title')}</h1>
    <p class="summary" aria-live="off">
      {#if summary.active > 0}
        <span class="speed"><Icon name="down" size={15} />{i18n.speed(summary.speed)}</span>
        · {t('downloads.summary.active', { n: summary.active })}{summary.queued > 0
          ? ` · ${t('downloads.summary.queued', { n: summary.queued })}`
          : ''}
      {:else if summary.queued > 0}
        {t('downloads.summary.queued', { n: summary.queued })}
      {:else}
        {t('downloads.summary.idle')}
      {/if}
    </p>
  </div>
  {#if finished > 0}
    <button class="btn" onclick={clearFinished}>
      <Icon name="trash" size={17} />{t('downloads.clearFinished', { n: finished })}
    </button>
  {/if}
</header>

<div class="toolbar">
  <div class="search">
    <Icon name="search" size={18} />
    <label class="sr-only" for="q">{t('downloads.search.label')}</label>
    <input
      id="q"
      class="input"
      type="search"
      placeholder={t('downloads.search.placeholder')}
      autocomplete="off"
      bind:value={prefs.query}
    />
  </div>

  <div class="sort">
    <label class="sr-only" for="sort">{t('downloads.sort.label')}</label>
    <select id="sort" class="select" value={prefs.sort.key} onchange={(e) => prefs.setSortKey(e.currentTarget.value as SortKey)}>
      {#each SORT_OPTIONS as o (o.key)}
        <option value={o.key}>{t(`sort.${o.key}`)}</option>
      {/each}
    </select>
    <button
      class="btn icon"
      onclick={() => prefs.toggleSortDir()}
      aria-label={t('downloads.sort.direction', { direction: sortLabel })}
      title={sortLabel}
    >
      <Icon name={prefs.sort.dir === 'asc' ? 'up' : 'down'} />
    </button>
  </div>
</div>

<!-- The side panel shows the filters on wide screens; on narrow ones they sit in the menu, so
     the ones in effect are listed here and can be removed with one tap. -->
{#if prefs.group !== 'all' || prefs.category !== 'all'}
  <div class="applied" role="group" aria-label={t('filters.active')}>
    {#if prefs.group !== 'all'}
      {@const name = t(`group.${prefs.group}`)}
      <button class="tag" aria-label={t('filters.remove', { name })} onclick={() => prefs.setGroup('all')}>
        {name}<Icon name="x" size={14} />
      </button>
    {/if}
    {#if prefs.category !== 'all'}
      {@const name = t(`category.${prefs.category}`)}
      <button class="tag" aria-label={t('filters.remove', { name })} onclick={() => prefs.setCategory('all')}>
        {name}<Icon name="x" size={14} />
      </button>
    {/if}
  </div>
{/if}

{#if !downloads.loaded && !downloads.loadError}
  <p class="empty" role="status">{t('common.loading')}</p>
{:else if downloads.loadError && !downloads.loaded}
  <div class="empty" role="alert">
    <p>{t('downloads.loadFailed', { error: i18n.errorText(downloads.loadError) })}</p>
    <button class="btn" onclick={() => downloads.reload()}>{t('common.tryAgain')}</button>
  </div>
{:else if downloads.items.length === 0}
  <div class="empty">
    <Icon name="down" size={32} />
    <p>{t('downloads.empty')}</p>
    <a class="btn primary" href="#/add"><Icon name="plus" size={18} />{t('downloads.empty.add')}</a>
  </div>
{:else if visible.length === 0}
  <div class="empty">
    <p>{t('downloads.noMatch')}</p>
    {#if filtering}<button class="btn" onclick={resetFilters}>{t('downloads.clearFilters')}</button>{/if}
  </div>
{:else}
  <ul class="list">
    {#each visible as d (d.id)}
      <li><DownloadCard download={d} /></li>
    {/each}
  </ul>
{/if}

<style>
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    margin-bottom: 1rem;
  }

  .head .btn {
    white-space: nowrap;
  }

  .summary {
    margin-top: 0.25rem;
    color: var(--muted);
    font-size: 0.9rem;
    font-variant-numeric: tabular-nums;
  }

  .speed {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    color: var(--accent);
    font-weight: 600;
  }

  .toolbar {
    display: grid;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
  }

  @media (min-width: 40rem) {
    .toolbar {
      grid-template-columns: minmax(0, 1fr) auto;
    }
  }

  .search {
    position: relative;
  }

  .search :global(svg) {
    position: absolute;
    left: 0.8rem;
    top: 50%;
    transform: translateY(-50%);
    color: var(--muted);
    pointer-events: none;
  }

  .search .input {
    padding-left: 2.4rem;
  }

  .sort {
    display: flex;
    gap: 0.5rem;
  }

  .sort .select {
    flex: 1;
    min-width: 9rem;
  }

  .applied {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-bottom: 0.75rem;
  }

  .tag {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    min-height: 2.25rem;
    padding: 0.2rem 0.6rem 0.2rem 0.8rem;
    border: 1px solid var(--accent);
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }

  /* On wide screens the side panel already shows what is selected. */
  @media (min-width: 64rem) {
    .applied {
      display: none;
    }
  }

  .list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 25rem), 1fr));
    gap: 0.75rem;
    margin: 1rem 0 0;
    padding: 0;
    list-style: none;
  }

  .empty {
    display: grid;
    justify-items: center;
    gap: 0.75rem;
    margin-top: 2.5rem;
    color: var(--muted);
    text-align: center;
  }
</style>
