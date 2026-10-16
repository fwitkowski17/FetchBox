<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    open: boolean;
    title: string;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
    /** `lg` for dialogs with a form, `sm` for a short notice. */
    size?: 'sm' | 'md' | 'lg';
    /** `false`: Escape and a click on the backdrop do nothing (a notice that only the app can end). */
    dismissible?: boolean;
  }

  let { open, title, onclose, children, footer, size = 'md', dismissible = true }: Props = $props();

  let dialog: HTMLDialogElement | undefined = $state();
  const titleId = `modal-title-${Math.random().toString(36).slice(2, 8)}`;

  // The native <dialog> traps focus, handles Escape and inerts the page behind it. Opening plays
  // the CSS animation at once; closing plays the exit first (`data-closing`) and only then closes
  // the element, since a closed <dialog> disappears immediately.
  let closing = 0;
  $effect(() => {
    if (!dialog) return;
    const el = dialog;
    if (open) {
      closing++; // cancels an exit that is still running
      delete el.dataset.closing;
      if (!el.open) el.showModal();
    } else if (el.open) {
      const mine = ++closing;
      el.dataset.closing = '';
      void Promise.allSettled(el.getAnimations().map((a) => a.finished)).then(() => {
        if (mine !== closing) return; // reopened meanwhile
        delete el.dataset.closing;
        el.close();
      });
    }
  });
</script>

<!-- Keyboard users close it with Escape (the `cancel` event); the click is for the backdrop. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialog}
  data-modal
  aria-labelledby={titleId}
  class={[
    'm-auto max-h-[calc(100dvh-2rem)] rounded-2xl focus-visible:outline-none border border-border bg-surface p-0 text-text shadow-card backdrop:bg-backdrop',
    size === 'lg'
      ? 'w-[min(36rem,calc(100vw-2rem))]'
      : size === 'sm'
        ? 'w-[min(22rem,calc(100vw-2rem))]'
        : 'w-[min(30rem,calc(100vw-2rem))]',
  ]}
  oncancel={(e) => {
    e.preventDefault();
    if (dismissible) onclose();
  }}
  onclick={(e) => {
    if (dismissible && e.target === dialog) onclose();
  }}
>
  <div class="grid grid-cols-[minmax(0,1fr)] gap-4 p-5">
    <h2 id={titleId} class="text-lg font-semibold tracking-tight">{title}</h2>
    <div class="grid grid-cols-[minmax(0,1fr)] gap-3">{@render children()}</div>
    {#if footer}
      <!-- Stays in view when a long dialog scrolls, so its buttons are never below the fold. -->
      <div
        class="sticky bottom-0 z-10 -mx-5 -mb-5 flex flex-wrap justify-end gap-2 bg-surface px-5 pt-3 pb-5 max-sm:flex-col-reverse max-sm:*:w-full"
      >
        {@render footer()}
      </div>
    {/if}
  </div>
</dialog>
