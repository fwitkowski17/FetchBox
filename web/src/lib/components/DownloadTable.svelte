<script lang="ts">
  // The list as a table: one row per download, columns that can be clicked to sort. Narrow screens
  // drop the columns they have no room for (the progress moves under the name).
  import type { Download } from '../api/types';
  import { t } from '../i18n';
  import { prefs } from '../stores/prefs.svelte';
  import { selection } from '../stores/selection.svelte';
  import type { SortKey } from '../util/downloads';
  import DownloadRow from './DownloadRow.svelte';
  import Icon from './Icon.svelte';

  let { items }: { items: readonly Download[] } = $props();

  /** A new column starts in its natural direction; the same one again reverses it. */
  function sortBy(key: SortKey): void {
    if (prefs.sort.key === key) prefs.toggleSortDir();
    else prefs.setSortKey(key);
  }

  const COLUMNS: readonly { key: SortKey; class: string; align: 'left' | 'right' }[] = [
    { key: 'name', class: 'w-full', align: 'left' },
    { key: 'status', class: 'max-sm:hidden', align: 'left' },
    { key: 'progress', class: 'max-md:hidden min-w-40', align: 'left' },
    { key: 'size', class: 'max-lg:hidden w-28', align: 'right' },
    { key: 'speed', class: 'max-lg:hidden w-28', align: 'right' },
  ];
</script>

<div class="rounded-2xl border border-border bg-surface shadow-card" data-part="table">
  <table class="w-full border-collapse text-left" aria-label={t('downloads.title')}>
    <thead>
      <tr class="text-xs font-semibold tracking-wide text-muted uppercase">
        {#if selection.active}<th class="w-10" scope="col"><span class="sr-only">{t('downloads.select')}</span></th>{/if}
        {#each COLUMNS as c (c.key)}
          {@const current = prefs.sort.key === c.key}
          <th
            scope="col"
            class={['px-3 py-2 whitespace-nowrap', c.class, c.align === 'right' && 'text-right']}
            aria-sort={current ? (prefs.sort.dir === 'asc' ? 'ascending' : 'descending') : undefined}
          >
            <button
              type="button"
              class={[
                'inline-flex min-h-9 items-center gap-1 rounded-md font-semibold tracking-wide uppercase hover:text-text focus-visible:text-text',
                current && 'text-text',
              ]}
              onclick={() => sortBy(c.key)}
              data-part="sort"
              data-key={c.key}
            >
              {t(`sort.${c.key}`)}
              {#if current}<Icon name={prefs.sort.dir === 'asc' ? 'up' : 'down'} size={13} />{/if}
            </button>
          </th>
        {/each}
        <!-- The speed column's neighbour: time left has no sort of its own. -->
        <th scope="col" class="px-3 py-2 text-right whitespace-nowrap max-lg:hidden">{t('downloads.col.eta')}</th>
        <th scope="col" class="w-px px-3 py-2"><span class="sr-only">{t('downloads.col.actions')}</span></th>
      </tr>
    </thead>
    <tbody>
      {#each items as d (d.id)}
        <DownloadRow download={d} />
      {/each}
    </tbody>
  </table>
</div>
