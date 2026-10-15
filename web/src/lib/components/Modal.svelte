<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    open: boolean;
    title: string;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
    /** `lg` for dialogs with a form. */
    size?: 'md' | 'lg';
  }

  let { open, title, onclose, children, footer, size = 'md' }: Props = $props();

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
    'm-auto max-h-[calc(100dvh-2rem)] rounded-2xl border border-border bg-surface p-0 text-text shadow-card backdrop:bg-backdrop',
    size === 'lg' ? 'w-[min(36rem,calc(100vw-2rem))]' : 'w-[min(30rem,calc(100vw-2rem))]',
  ]}
  oncancel={(e) => {
    e.preventDefault();
    onclose();
  }}
  onclick={(e) => {
    if (e.target === dialog) onclose();
  }}
>
  <div class="grid gap-4 p-5">
    <h2 id={titleId} class="text-lg font-semibold tracking-tight">{title}</h2>
    <div class="grid gap-3">{@render children()}</div>
    {#if footer}
      <div class="flex flex-wrap justify-end gap-2 max-sm:flex-col-reverse max-sm:*:w-full">
        {@render footer()}
      </div>
    {/if}
  </div>
</dialog>
