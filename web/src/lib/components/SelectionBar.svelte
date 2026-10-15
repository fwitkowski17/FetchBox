<script lang="ts">
  // The action bar while several downloads are selected: change the category, archive (or
  // restore) and delete. Sits at the bottom of the screen, next to the side panel on wide ones.
  import { fly } from 'svelte/transition';
  import type { Category, Download } from '../api/types';
  import { CATEGORIES } from '../api/types';
  import { i18n, t } from '../i18n';
  import { deleteDialog } from '../stores/deleteDialog.svelte';
  import { downloads } from '../stores/downloads.svelte';
  import { selection } from '../stores/selection.svelte';
  import { toast } from '../stores/toast.svelte';
  import { bulkInfo, pickSelected, type BulkResult } from '../util/bulk';
  import { motion } from '../util/motion';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  /** What the list shows now: "select all" means these. */
  let { visible }: { visible: readonly Download[] } = $props();

  const selected = $derived(pickSelected(downloads.items, selection.ids));
  const info = $derived(bulkInfo(selected));
  const allSelected = $derived(visible.length > 0 && visible.every((d) => selection.has(d.id)));
  /** With nothing left to archive but something already archived, the button brings them back. */
  const restoring = $derived(info.archivable === 0 && info.archived > 0);
  const canArchive = $derived(info.archivable > 0 || info.archived > 0);

  let categoryOpen = $state(false);
  let busy = $state(false);

  const BUTTON = 'btn whitespace-nowrap max-sm:flex-1 max-sm:px-0';

  /** One summary for a bulk run; a failure shows what the first one said. */
  function report(result: BulkResult, doneMessage: string): void {
    if (result.failed > 0) {
      toast.error(`${t('bulk.failed', { n: result.failed })}: ${i18n.errorText(result.firstError)}`);
    }
    if (result.done > 0) toast.success(doneMessage);
  }

  async function run(work: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await work();
    } finally {
      busy = false;
    }
  }

  const toggleAll = (): void =>
    allSelected ? selection.clear() : selection.setAll(visible.map((d) => d.id));

  async function setCategory(category: Category): Promise<void> {
    categoryOpen = false;
    const ids = selected.filter((d) => d.category !== category).map((d) => d.id);
    if (ids.length === 0) return;
    await run(async () => {
      const result = await downloads.setCategoryMany(ids, category);
      report(result, t('bulk.category.done', { n: result.done }));
    });
  }

  async function archive(): Promise<void> {
    const toArchive = restoring
      ? selected.filter((d) => d.archived)
      : selected.filter(
          (d) => !d.archived && ['completed', 'failed', 'cancelled'].includes(d.status),
        );
    await run(async () => {
      const result = await downloads.setArchivedMany(
        toArchive.map((d) => d.id),
        !restoring,
      );
      report(result, t(restoring ? 'bulk.restored' : 'bulk.archived', { n: result.done }));
      const skipped = restoring ? 0 : info.unfinished;
      if (skipped > 0) toast.info(t('bulk.archiveSkipped', { n: skipped }));
      // Archived entries leave this list: the selection is empty now.
      selection.clear();
    });
  }

  function remove(): void {
    deleteDialog.show(...selected.map((d) => d.id));
  }
</script>

<svelte:window
  onkeydown={(e) => {
    // Escape leaves the selection, unless a dialog (which uses Escape itself) is open.
    if (e.key === 'Escape' && !document.querySelector('dialog[open]')) selection.exit();
  }}
/>

<div
  class="fixed inset-x-0 bottom-0 z-20 border-t border-border bg-surface px-3 pt-2 pb-[calc(0.5rem+env(safe-area-inset-bottom))] shadow-[0_-6px_20px_rgb(0_0_0/0.08)] lg:left-[var(--sidebar-w)]"
  role="toolbar"
  aria-label={t('selection.label')}
  data-part="selection-bar"
  transition:fly={{ y: 40, duration: motion(180) }}
>
  <div class="mx-auto flex max-w-[76rem] flex-wrap items-center gap-x-2 gap-y-2 lg:px-5">
    <!-- On a phone: the count on top, the actions below, evenly spread. -->
    <div class="flex items-center gap-2 max-sm:w-full max-sm:justify-between">
      <p class="mr-1 font-semibold tabular-nums" role="status" data-part="selection-count">
        {t('selection.count', { n: selection.size })}
      </p>
      <button class="btn ghost whitespace-nowrap text-accent" onclick={toggleAll} data-part="select-all">
        {allSelected ? t('selection.none') : t('selection.all')}
      </button>
    </div>

    <span class="flex-1 max-sm:hidden"></span>

    <div class="flex items-center gap-2 max-sm:w-full">
      <button
        class={BUTTON}
        disabled={selection.size === 0 || busy}
        onclick={() => (categoryOpen = true)}
        aria-label={t('selection.category')}
        title={t('selection.category')}
        data-part="bulk-category"
      >
        <Icon name="tag" size={18} /><span class="max-sm:sr-only">{t('selection.category')}</span>
      </button>
      <button
        class={BUTTON}
        disabled={!canArchive || busy}
        onclick={archive}
        aria-label={restoring ? t('selection.restore') : t('selection.archive')}
        title={restoring ? t('selection.restore') : t('selection.archive')}
        data-part="bulk-archive"
      >
        <Icon name={restoring ? 'up' : 'archive'} size={18} />
        <span class="max-sm:sr-only">{restoring ? t('selection.restore') : t('selection.archive')}</span>
      </button>
      <button
        class={[BUTTON, 'danger']}
        disabled={selection.size === 0 || busy}
        onclick={remove}
        aria-label={t('selection.delete')}
        title={t('selection.delete')}
        data-part="bulk-delete"
      >
        <Icon name="trash" size={18} /><span class="max-sm:sr-only">{t('selection.delete')}</span>
      </button>
      <button
        class="btn primary whitespace-nowrap max-sm:flex-[1.4]"
        onclick={() => selection.exit()}
        data-part="selection-done"
      >
        {t('selection.done')}
      </button>
    </div>
  </div>
</div>

<Modal open={categoryOpen} title={t('bulk.category.title')} onclose={() => (categoryOpen = false)}>
  <p class="hint">{t('bulk.category.text', { n: selection.size })}</p>
  <div class="grid grid-cols-2 gap-2">
    {#each CATEGORIES as c (c)}
      <button class="btn justify-start" onclick={() => void setCategory(c)} data-category={c}>
        <CategoryIcon category={c} size={18} />{t(`category.${c}`)}
      </button>
    {/each}
  </div>
  {#snippet footer()}
    <button class="btn" onclick={() => (categoryOpen = false)}>{t('common.cancel')}</button>
  {/snippet}
</Modal>
