<script lang="ts">
  import CategoryIcon from '../lib/components/CategoryIcon.svelte';
  import DownloadCard from '../lib/components/DownloadCard.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { i18n, t } from '../lib/i18n';
  import { downloads } from '../lib/stores/downloads.svelte';
  import { prefs } from '../lib/stores/prefs.svelte';
  import { toast } from '../lib/stores/toast.svelte';
  import { GROUP_COLOR, GROUP_ICON } from '../lib/util/groupStyle';
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

<header class="mb-4 flex flex-wrap items-end justify-between gap-x-4 gap-y-2">
  <div>
    <h1 class="text-2xl font-bold tracking-tight">{t('downloads.title')}</h1>
    <p class="mt-1 text-sm text-muted tabular-nums" aria-live="off" data-part="summary">
      {#if summary.active > 0}
        <span class="inline-flex items-center gap-0.5 font-semibold text-accent">
          <Icon name="down" size={15} />{i18n.speed(summary.speed)}
        </span>
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
    <button class="btn whitespace-nowrap" onclick={clearFinished}>
      <Icon name="trash" size={17} />{t('downloads.clearFinished', { n: finished })}
    </button>
  {/if}
</header>

<div class="mb-3 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
  <div class="relative">
    <span class="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-muted">
      <Icon name="search" size={18} />
    </span>
    <label class="sr-only" for="q">{t('downloads.search.label')}</label>
    <input
      id="q"
      class="input pl-10"
      type="search"
      placeholder={t('downloads.search.placeholder')}
      autocomplete="off"
      bind:value={prefs.query}
    />
  </div>

  <div class="flex gap-2">
    <label class="sr-only" for="sort">{t('downloads.sort.label')}</label>
    <select
      id="sort"
      class="select min-w-36 flex-1"
      value={prefs.sort.key}
      onchange={(e) => prefs.setSortKey(e.currentTarget.value as SortKey)}
    >
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
  <div class="mb-3 flex flex-wrap gap-1.5 lg:hidden" role="group" aria-label={t('filters.active')}>
    {#each [prefs.group !== 'all' ? { name: t(`group.${prefs.group}`), group: prefs.group, clear: () => prefs.setGroup('all') } : null, prefs.category !== 'all' ? { name: t(`category.${prefs.category}`), category: prefs.category, clear: () => prefs.setCategory('all') } : null] as tag (tag?.name)}
      {#if tag}
        <button
          class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-accent bg-accent-soft py-0.5 pr-2.5 pl-3.5 text-sm font-semibold text-accent"
          aria-label={t('filters.remove', { name: tag.name })}
          onclick={tag.clear}
        >
          {#if tag.group}
            <span class={GROUP_COLOR[tag.group]}><Icon name={GROUP_ICON[tag.group]} size={16} /></span>
          {:else if tag.category}
            <CategoryIcon category={tag.category} size={16} />
          {/if}
          {tag.name}<Icon name="x" size={14} />
        </button>
      {/if}
    {/each}
  </div>
{/if}

{#if !downloads.loaded && !downloads.loadError}
  <p class="mt-10 text-center text-muted" role="status">{t('common.loading')}</p>
{:else if downloads.loadError && !downloads.loaded}
  <div class="mt-10 grid justify-items-center gap-3 text-center text-muted" role="alert">
    <p>{t('downloads.loadFailed', { error: i18n.errorText(downloads.loadError) })}</p>
    <button class="btn" onclick={() => downloads.reload()}>{t('common.tryAgain')}</button>
  </div>
{:else if downloads.items.length === 0}
  <div class="mt-12 grid justify-items-center gap-3 text-center text-muted">
    <span class="grid size-16 place-items-center rounded-full bg-surface-2"><Icon name="down" size={28} /></span>
    <p>{t('downloads.empty')}</p>
    <a class="btn primary" href="#/add"><Icon name="plus" size={18} />{t('downloads.empty.add')}</a>
  </div>
{:else if visible.length === 0}
  <div class="mt-12 grid justify-items-center gap-3 text-center text-muted">
    <p>{t('downloads.noMatch')}</p>
    {#if filtering}<button class="btn" onclick={resetFilters}>{t('downloads.clearFilters')}</button>{/if}
  </div>
{:else}
  <ul class="mt-4 grid grid-cols-[repeat(auto-fill,minmax(min(100%,25rem),1fr))] gap-3">
    {#each visible as d (d.id)}
      <li><DownloadCard download={d} /></li>
    {/each}
  </ul>
{/if}
