<script lang="ts">
  // The "more" menu of a download card: copy the link, change the category, delete. A small
  // dropdown that opens upwards (the button sits at the bottom of the card), closes on Escape or
  // an outside click, and is operable with the arrow keys.
  import { tick } from 'svelte';
  import { scale } from 'svelte/transition';
  import type { Category, Download } from '../api/types';
  import { CATEGORIES } from '../api/types';
  import { t } from '../i18n';
  import { deleteDialog } from '../stores/deleteDialog.svelte';
  import { downloads } from '../stores/downloads.svelte';
  import { toast } from '../stores/toast.svelte';
  import { copyText } from '../util/clipboard';
  import { motion } from '../util/motion';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';

  let { download, disabled = false }: { download: Download; disabled?: boolean } = $props();

  /** Panel size used to decide where it fits (the category list is the tallest view). */
  const PANEL_W = 240;
  const PANEL_H = 330;

  let open = $state(false);
  /** Which way the panel opens, decided when it opens from where the button is on screen. */
  let up = $state(true);
  let alignRight = $state(false);
  let view = $state<'main' | 'category'>('main');
  let root: HTMLDivElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();
  let panel: HTMLDivElement | undefined = $state();

  const ITEM =
    'flex min-h-10 w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left font-medium hover:bg-surface-2 focus-visible:bg-surface-2 focus-visible:outline-none aria-checked:text-accent';

  function items(): HTMLElement[] {
    return [...(panel?.querySelectorAll<HTMLElement>('[role^="menuitem"]') ?? [])];
  }

  /** Open towards the side with room: the button is at the bottom of a card that can be anywhere
   *  on the screen, and a card at the left edge must not push the panel off it. */
  function place(): void {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    const room = { above: r.top, below: window.innerHeight - r.bottom };
    up = room.above >= PANEL_H || room.above >= room.below;
    const fitsRight = r.right - PANEL_W >= 8; // right edges aligned, panel extends to the left
    const fitsLeft = r.left + PANEL_W <= window.innerWidth - 8;
    alignRight = fitsRight && !(fitsLeft && r.left < window.innerWidth / 2);
  }

  async function show(): Promise<void> {
    place();
    view = 'main';
    open = true;
    await tick();
    items()[0]?.focus();
  }

  function close(refocus = true): void {
    open = false;
    if (refocus) trigger?.focus();
  }

  async function goto(next: 'main' | 'category'): Promise<void> {
    view = next;
    await tick();
    // In the category list start on the current one.
    const current = panel?.querySelector<HTMLElement>('[aria-checked="true"]');
    (current ?? items()[0])?.focus();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (!open) return;
    const list = items();
    const at = list.indexOf(document.activeElement as HTMLElement);
    const focus = (i: number) => list[(i + list.length) % list.length]?.focus();
    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        e.stopPropagation();
        if (view === 'category') void goto('main');
        else close();
        break;
      case 'ArrowDown':
        e.preventDefault();
        focus(at + 1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        focus(at < 0 ? -1 : at - 1);
        break;
      case 'Home':
        e.preventDefault();
        focus(0);
        break;
      case 'End':
        e.preventDefault();
        focus(-1);
        break;
      case 'Tab':
        close(false); // leave the menu where Tab takes the focus
        break;
    }
  }

  async function copyLink(): Promise<void> {
    close();
    if (await copyText(download.url)) toast.success(t('menu.copied'));
    else toast.error(t('menu.copyFailed'));
  }

  async function pick(category: Category): Promise<void> {
    close();
    if (category !== download.category) await downloads.setCategory(download.id, category);
  }

  const archivable = $derived(['completed', 'failed', 'cancelled'].includes(download.status));

  async function toggleArchived(): Promise<void> {
    close();
    const archive = !download.archived;
    if (await downloads.setArchived(download.id, archive)) {
      toast.success(t(archive ? 'bulk.archived' : 'bulk.restored', { n: 1 }));
    }
  }

  function remove(): void {
    close(false);
    deleteDialog.show(download.id);
  }
</script>

<svelte:window
  onpointerdown={(e) => {
    if (open && root && !root.contains(e.target as Node)) close(false);
  }}
/>

<div class="relative" bind:this={root}>
  <button
    bind:this={trigger}
    class="btn icon"
    type="button"
    {disabled}
    aria-haspopup="menu"
    aria-expanded={open}
    aria-label={t('menu.open', { name: download.filename })}
    title={t('menu.label')}
    onclick={() => (open ? close() : void show())}
  >
    <Icon name="more" />
  </button>

  {#if open}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      bind:this={panel}
      role="menu"
      aria-label={t('menu.label')}
      class={[
        'absolute z-30 w-60 rounded-xl border border-border bg-surface p-1 shadow-card',
        up ? 'bottom-full mb-2' : 'top-full mt-2',
        alignRight ? 'right-0' : 'left-0',
        up ? (alignRight ? 'origin-bottom-right' : 'origin-bottom-left') : alignRight ? 'origin-top-right' : 'origin-top-left',
      ]}
      transition:scale={{ start: 0.94, duration: motion(120) }}
      {onkeydown}
    >
      {#if view === 'main'}
        <button class={ITEM} role="menuitem" type="button" onclick={copyLink}>
          <Icon name="copy" size={18} />{t('menu.copyLink')}
        </button>
        <button class={ITEM} role="menuitem" type="button" onclick={() => void goto('category')}>
          <CategoryIcon category={download.category} size={18} />{t('menu.category')}
          <span class="ml-auto text-muted"><Icon name="chevronRight" size={16} /></span>
        </button>
        {#if download.archived || archivable}
          <button class={ITEM} role="menuitem" type="button" onclick={toggleArchived}>
            <Icon name={download.archived ? 'up' : 'archive'} size={18} />
            {download.archived ? t('menu.restore') : t('menu.archive')}
          </button>
        {/if}
        <hr class="m-1 border-0 border-t border-border" />
        <button class={[ITEM, 'text-danger']} role="menuitem" type="button" onclick={remove}>
          <Icon name="trash" size={18} />{t('menu.delete')}
        </button>
      {:else}
        <button class={ITEM} role="menuitem" type="button" onclick={() => void goto('main')}>
          <Icon name="chevronLeft" size={18} />{t('menu.back')}
        </button>
        <hr class="m-1 border-0 border-t border-border" />
        {#each CATEGORIES as c (c)}
          <button
            class={ITEM}
            role="menuitemradio"
            aria-checked={download.category === c}
            type="button"
            onclick={() => void pick(c)}
          >
            <CategoryIcon category={c} size={18} />{t(`category.${c}`)}
            {#if download.category === c}<span class="ml-auto"><Icon name="check" size={16} /></span>{/if}
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</div>
