<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    checked: boolean;
    children: Snippet;
    hint?: string;
  }

  let { checked = $bindable(), children, hint }: Props = $props();
</script>

<label class="flex min-h-11 cursor-pointer items-center justify-between gap-4">
  <span class="grid gap-0.5">
    <span class="font-semibold">{@render children()}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </span>
  <!-- The real checkbox lies, invisible, over the whole track: it is what gets clicked and
       focused, and what assistive technology announces as a switch. -->
  <span class="relative shrink-0">
    <input
      type="checkbox"
      role="switch"
      class="peer absolute inset-0 z-10 size-full cursor-pointer opacity-0"
      bind:checked
    />
    <span
      class="block h-7 w-12 rounded-full bg-border p-0.5 transition-colors peer-checked:bg-accent peer-checked:*:translate-x-5 peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-accent"
      aria-hidden="true"
    >
      <span class="block size-6 rounded-full bg-white shadow-sm transition-transform"></span>
    </span>
  </span>
</label>
