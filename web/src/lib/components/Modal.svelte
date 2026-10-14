<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    open: boolean;
    title: string;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
  }

  let { open, title, onclose, children, footer }: Props = $props();

  let dialog: HTMLDialogElement | undefined = $state();
  const titleId = `modal-title-${Math.random().toString(36).slice(2, 8)}`;

  // The native <dialog> traps focus, handles Escape and inerts the page behind it.
  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });
</script>

<!-- Keyboard users close it with Escape (the `cancel` event); the click is for the backdrop. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  oncancel={(e) => {
    e.preventDefault();
    onclose();
  }}
  onclick={(e) => {
    if (e.target === dialog) onclose();
  }}
>
  <div class="panel">
    <h2 id={titleId}>{title}</h2>
    <div class="body">{@render children()}</div>
    {#if footer}
      <div class="footer">{@render footer()}</div>
    {/if}
  </div>
</dialog>

<style>
  dialog {
    width: min(30rem, calc(100vw - 2rem));
    max-height: calc(100dvh - 2rem);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  dialog::backdrop {
    background: var(--backdrop);
  }

  .panel {
    display: grid;
    gap: 1rem;
    padding: 1.25rem;
  }

  h2 {
    font-size: 1.15rem;
  }

  .body {
    display: grid;
    gap: 0.75rem;
  }

  .footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.5rem;
  }

  @media (max-width: 30rem) {
    .footer {
      flex-direction: column-reverse;
    }
    .footer :global(.btn) {
      width: 100%;
    }
  }
</style>
