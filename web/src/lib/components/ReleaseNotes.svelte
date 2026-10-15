<script lang="ts">
  // Release notes as structure made of text nodes (see `util/notes`): nothing from GitHub is ever
  // interpreted as HTML.
  import type { NoteBlock, Segment } from '../util/notes';

  let { blocks }: { blocks: NoteBlock[] } = $props();
</script>

{#snippet inline(segments: Segment[])}
  {#each segments as s, i (i)}
    {#if s.code}<code class="rounded bg-surface px-1 font-mono text-[0.85em]">{s.text}</code
      >{:else if s.bold}<strong>{s.text}</strong>{:else}{s.text}{/if}
  {/each}
{/snippet}

<div class="grid gap-1.5 text-sm [overflow-wrap:anywhere]">
  {#each blocks as b, i (i)}
    {#if b.kind === 'heading'}
      <p class="mt-1 font-semibold">{b.text}</p>
    {:else if b.kind === 'list'}
      <ul class="grid list-disc gap-0.5 pl-5">
        {#each b.items as item, j (j)}
          <li>{@render inline(item)}</li>
        {/each}
      </ul>
    {:else}
      <p>{@render inline(b.segments)}</p>
    {/if}
  {/each}
</div>
