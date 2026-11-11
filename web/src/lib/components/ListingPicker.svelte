<script lang="ts">
  import type { SvelteSet } from 'svelte/reactivity';
  import type { ListingEntry } from '../api/types';
  import { i18n, t } from '../i18n';
  import { plugins } from '../stores/plugins.svelte';
  import { entryKey, selectionTotal, visibleEntries } from '../util/listing';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';

  interface Props {
    entries: ListingEntry[];
    /** The page had more items than are shown. */
    truncated: boolean;
    /** Keys (`entryKey`) of the chosen files. */
    chosen: SvelteSet<string>;
    /** Whether there is a folder to go back to. */
    canGoBack: boolean;
    disabled?: boolean;
    onopen: (url: string) => void;
    onback: () => void;
  }

  let { entries, truncated, chosen, canGoBack, disabled = false, onopen, onback }: Props = $props();

  let query = $state('');
  const shown = $derived(visibleEntries(entries, query));
  const shownFiles = $derived(shown.filter((e) => !e.is_dir));
  const total = $derived(selectionTotal(entries, chosen));
  const allShownChosen = $derived(shownFiles.length > 0 && shownFiles.every((e) => chosen.has(entryKey(e))));
  const someShownChosen = $derived(shownFiles.some((e) => chosen.has(entryKey(e))));

  function toggleAll(): void {
    if (allShownChosen) for (const e of shownFiles) chosen.delete(entryKey(e));
    else for (const e of shownFiles) chosen.add(entryKey(e));
  }

  function toggle(key: string): void {
    if (chosen.has(key)) chosen.delete(key);
    else chosen.add(key);
  }

  let allBox = $state<HTMLInputElement>();
  $effect(() => {
    if (allBox) allBox.indeterminate = someShownChosen && !allShownChosen;
  });
</script>

<section class="grid gap-2 rounded-xl border border-border" aria-label={t('add.listing.title')} data-part="listing">
  <div class="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2">
    {#if canGoBack}
      <button type="button" class="btn ghost" {disabled} onclick={onback} data-part="listing-back">
        <Icon name="chevronLeft" size={18} />{t('add.listing.back')}
      </button>
    {/if}
    <label class="flex min-h-9 items-center gap-2 font-semibold">
      <input
        bind:this={allBox}
        type="checkbox"
        class="size-5 accent-[var(--accent)]"
        checked={allShownChosen}
        disabled={disabled || shownFiles.length === 0}
        onchange={toggleAll}
        data-part="listing-all"
      />
      {t('selection.all')}
    </label>
    <input
      class="input ml-auto w-full min-w-0 sm:w-48"
      type="search"
      autocomplete="off"
      placeholder={t('add.listing.filter')}
      aria-label={t('add.listing.filter')}
      bind:value={query}
    />
  </div>

  <ul class="max-h-[min(18rem,30dvh)] overflow-y-auto px-1" data-part="listing-items">
    {#each shown as e (entryKey(e))}
      <li>
        {#if e.is_dir}
          <button
            type="button"
            class="flex min-h-11 w-full items-center gap-3 rounded-lg px-2 text-left hover:bg-surface-2"
            {disabled}
            aria-label={t('add.listing.openFolder', { name: e.name })}
            onclick={() => onopen(e.url)}
            data-part="listing-folder"
          >
            <span class="grid size-6 shrink-0 place-items-center text-muted"><Icon name="folder" size={20} /></span>
            <span class="min-w-0 flex-1 truncate font-medium" title={e.name}>{e.name}</span>
            <Icon name="chevronRight" size={18} />
          </button>
        {:else}
          <label class="flex min-h-11 cursor-pointer items-center gap-3 rounded-lg px-2 hover:bg-surface-2">
            <input
              type="checkbox"
              class="size-5 shrink-0 accent-[var(--accent)]"
              checked={chosen.has(entryKey(e))}
              {disabled}
              onchange={() => toggle(entryKey(e))}
              aria-label={t('selection.card', { name: e.name })}
              data-part="listing-file"
            />
            <CategoryIcon category={e.category} size={20} />
            <span class="min-w-0 flex-1 truncate" title={e.name}>{e.name}</span>
            {#if e.detail}
              <span class="max-w-[45%] shrink-0 truncate rounded-md bg-accent-soft px-1.5 py-0.5 text-xs text-accent" data-part="listing-detail" title={e.detail}>{e.detail}</span>
            {/if}
            {#if e.via}
              <span class="hidden shrink-0 rounded-md bg-surface-2 px-1.5 py-0.5 text-xs text-muted sm:inline" data-part="listing-via">
                {t('add.via.badge', { plugin: plugins.get(e.via.plugin)?.name ?? e.via.plugin })}
              </span>
            {/if}
            {#if e.size !== null && e.size !== undefined}
              <span class="shrink-0 text-sm text-muted tabular-nums">{i18n.bytes(e.size)}</span>
            {/if}
          </label>
        {/if}
      </li>
    {:else}
      <li class="px-3 py-4 text-center text-muted">
        {entries.length === 0 ? t('add.listing.empty') : t('add.listing.noMatch')}
      </li>
    {/each}
  </ul>

  <div class="grid gap-0.5 border-t border-border px-3 py-2 text-sm" aria-live="polite">
    <p class="flex flex-wrap items-center gap-x-3">
      <span class="font-semibold" data-part="listing-count">{t('selection.count', { n: total.count })}</span>
      {#if total.sized > 0}
        <span class="text-muted tabular-nums" data-part="listing-total">
          {total.partial
            ? t('add.listing.totalPartial', { size: i18n.bytes(total.bytes) })
            : t('add.listing.total', { size: i18n.bytes(total.bytes) })}
        </span>
      {/if}
    </p>
    {#if truncated}
      <p class="text-warning">{t('add.listing.truncated', { count: entries.length })}</p>
    {/if}
  </div>
</section>
