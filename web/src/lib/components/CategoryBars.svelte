<script lang="ts">
  // Completed downloads per category, as horizontal bars: the name (with its icon) on the left,
  // the size at the tip. One hue, because the labels carry the identity; the longest bar is the
  // biggest category.
  import type { CategoryStat } from '../api/types';
  import { i18n, t } from '../i18n';
  import CategoryIcon from './CategoryIcon.svelte';

  let { categories }: { categories: readonly CategoryStat[] } = $props();

  const max = $derived(Math.max(1, ...categories.map((c) => c.bytes)));
</script>

<ul class="grid gap-3" data-part="category-bars">
  {#each categories as c (c.category)}
    <li class="grid grid-cols-[minmax(6.5rem,8rem)_1fr] items-center gap-x-3 gap-y-1 sm:grid-cols-[8rem_1fr_auto]">
      <span class="flex items-center gap-2 truncate">
        <CategoryIcon category={c.category} size={18} />
        <span class="truncate">{t(`category.${c.category}`)}</span>
      </span>
      <span class="h-3 rounded-full bg-surface-2" aria-hidden="true">
        <span
          class="block h-full rounded-r-[4px] rounded-l-full bg-accent transition-[width] duration-200"
          style:width="{Math.max(1.5, (c.bytes / max) * 100)}%"
        ></span>
      </span>
      <span class="col-span-2 text-sm tabular-nums sm:col-span-1 sm:text-right">
        <span class="font-semibold">{i18n.bytes(c.bytes)}</span>
        <span class="text-muted">· {t('stats.filesCount', { n: c.count })}</span>
      </span>
    </li>
  {/each}
</ul>
