<script lang="ts">
  // One number with its label and, if there is something to add, a line under it.
  import type { Snippet } from 'svelte';

  interface Props {
    label: string;
    /** The figure, already formatted. */
    value: string;
    /** A smaller line under it. */
    note?: string | undefined;
    /** What `value` is, for a screen reader, when the text alone is not enough. */
    title?: string | undefined;
    children?: Snippet;
  }

  let { label, value, note, title, children }: Props = $props();
</script>

<div class="grid content-start gap-1 rounded-xl border border-border bg-surface p-4" data-part="stat">
  <p class="text-sm font-medium text-muted">{label}</p>
  <p class="truncate text-2xl font-semibold tracking-tight" {title} data-part="stat-value">{value}</p>
  {#if note}<p class="line-clamp-2 text-sm break-words text-muted" title={note}>{note}</p>{/if}
  {@render children?.()}
</div>
