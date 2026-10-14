<script lang="ts">
  import type { Category } from '../lib/api/types';
  import { CATEGORIES, CATEGORY_LABELS } from '../lib/api/types';
  import CategoryIcon from '../lib/components/CategoryIcon.svelte';
  import DownloadCard from '../lib/components/DownloadCard.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { downloads } from '../lib/stores/downloads.svelte';
  import { prefs } from '../lib/stores/prefs.svelte';
  import { toast } from '../lib/stores/toast.svelte';
  import {
    GROUPS,
    SORT_OPTIONS,
    countCategories,
    countGroups,
    filterDownloads,
    sortDownloads,
    summarize,
    type SortKey,
  } from '../lib/util/downloads';
  import { formatSpeed } from '../lib/util/format';

  const filter = $derived({ group: prefs.group, category: prefs.category, query: prefs.query });
  const visible = $derived(sortDownloads(filterDownloads(downloads.items, filter), prefs.sort));
  const groupCounts = $derived(countGroups(downloads.items, filter));
  const categoryCounts = $derived(countCategories(downloads.items, filter));
  const summary = $derived(summarize(downloads.items));
  const finished = $derived(
    downloads.items.filter((d) => d.status === 'completed' || d.status === 'failed' || d.status === 'cancelled').length,
  );
  const filtering = $derived(prefs.group !== 'all' || prefs.category !== 'all' || prefs.query.trim() !== '');

  const sortLabel = $derived(prefs.sort.dir === 'asc' ? 'Ascending' : 'Descending');

  async function clearFinished(): Promise<void> {
    const n = finished;
    await downloads.clearFinished();
    toast.success(`Removed ${n} finished ${n === 1 ? 'entry' : 'entries'} from the list`);
  }

  function resetFilters(): void {
    prefs.setGroup('all');
    prefs.setCategory('all');
    prefs.query = '';
  }
</script>

<header class="head">
  <div>
    <h1>Downloads</h1>
    <p class="summary" aria-live="off">
      {#if summary.active > 0}
        <span class="speed"><Icon name="down" size={15} />{formatSpeed(summary.speed)}</span>
        · {summary.active} active{summary.queued > 0 ? ` · ${summary.queued} queued` : ''}
      {:else if summary.queued > 0}
        {summary.queued} queued
      {:else}
        Nothing running
      {/if}
    </p>
  </div>
  {#if finished > 0}
    <button class="btn" onclick={clearFinished}>
      <Icon name="trash" size={17} />Clear finished ({finished})
    </button>
  {/if}
</header>

<div class="toolbar">
  <div class="search">
    <Icon name="search" size={18} />
    <label class="sr-only" for="q">Search downloads</label>
    <input
      id="q"
      class="input"
      type="search"
      placeholder="Search by name or address"
      autocomplete="off"
      bind:value={prefs.query}
    />
  </div>

  <div class="sort">
    <label class="sr-only" for="sort">Sort by</label>
    <select id="sort" class="select" value={prefs.sort.key} onchange={(e) => prefs.setSortKey(e.currentTarget.value as SortKey)}>
      {#each SORT_OPTIONS as o (o.key)}
        <option value={o.key}>{o.label}</option>
      {/each}
    </select>
    <button
      class="btn icon"
      onclick={() => prefs.toggleSortDir()}
      aria-label={`Sort direction: ${sortLabel}. Activate to reverse.`}
      title={sortLabel}
    >
      <Icon name={prefs.sort.dir === 'asc' ? 'up' : 'down'} />
    </button>
  </div>
</div>

<div class="chips" role="group" aria-label="Filter by status">
  {#each GROUPS as g (g.id)}
    <button class="chip" aria-pressed={prefs.group === g.id} onclick={() => prefs.setGroup(g.id)}>
      {g.label}<span class="count">{groupCounts[g.id]}</span>
    </button>
  {/each}
</div>

<div class="chips" role="group" aria-label="Filter by category">
  <button class="chip" aria-pressed={prefs.category === 'all'} onclick={() => prefs.setCategory('all')}>
    All categories<span class="count">{categoryCounts.all}</span>
  </button>
  {#each CATEGORIES as c (c)}
    <button
      class="chip"
      aria-pressed={prefs.category === c}
      onclick={() => prefs.setCategory(c as Category)}
    >
      <CategoryIcon category={c} size={15} />{CATEGORY_LABELS[c]}<span class="count">{categoryCounts[c]}</span>
    </button>
  {/each}
</div>

{#if !downloads.loaded && !downloads.loadError}
  <p class="empty" role="status">Loading…</p>
{:else if downloads.loadError && !downloads.loaded}
  <div class="empty" role="alert">
    <p>Could not load the list: {downloads.loadError}</p>
    <button class="btn" onclick={() => downloads.reload()}>Try again</button>
  </div>
{:else if downloads.items.length === 0}
  <div class="empty">
    <Icon name="down" size={32} />
    <p>No downloads yet.</p>
    <a class="btn primary" href="#/add"><Icon name="plus" size={18} />Add a download</a>
  </div>
{:else if visible.length === 0}
  <div class="empty">
    <p>No downloads match{filtering ? ' these filters' : ''}.</p>
    {#if filtering}<button class="btn" onclick={resetFilters}>Clear filters</button>{/if}
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
      grid-template-columns: 1fr auto;
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

  .chips {
    display: flex;
    gap: 0.4rem;
    margin-bottom: 0.6rem;
    padding-bottom: 0.15rem;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .chips::-webkit-scrollbar {
    display: none;
  }

  /* Room on wide screens: wrap instead of scrolling sideways. */
  @media (min-width: 48rem) {
    .chips {
      flex-wrap: wrap;
      overflow-x: visible;
    }
  }

  .chip {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 0.35rem;
    min-height: 2.25rem;
    padding: 0.25rem 0.8rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }

  .chip[aria-pressed='true'] {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }

  .count {
    color: var(--muted);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .chip[aria-pressed='true'] .count {
    color: inherit;
  }

  .list {
    display: grid;
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
