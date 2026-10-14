<script lang="ts">
  import type { Conflict, OnConflict } from '../api/types';
  import { formatBytes, formatDate } from '../util/format';
  import Modal from './Modal.svelte';

  interface Props {
    conflict: Conflict | null;
    busy: boolean;
    /** `null` means the user backed out. */
    onchoose: (choice: OnConflict | null) => void;
  }

  let { conflict, busy, onchoose }: Props = $props();

  // A file name that is still being produced by another download cannot be replaced.
  const canOverwrite = $derived(conflict !== null && conflict.kind !== 'name_reserved');

  const title = $derived(
    conflict?.kind === 'duplicate_url' ? 'Already in your list' : 'File name already taken',
  );

  const existingDetails = $derived.by(() => {
    if (!conflict) return '';
    const parts: string[] = [];
    if (conflict.existing_size != null) parts.push(formatBytes(conflict.existing_size));
    if (conflict.existing_modified != null) parts.push(formatDate(conflict.existing_modified));
    return parts.join(' · ');
  });
</script>

<Modal open={conflict !== null} {title} onclose={() => !busy && onchoose(null)}>
  {#if conflict}
    {#if conflict.kind === 'duplicate_url'}
      <p>This address is already a download: <strong class="name">{conflict.existing_name}</strong>.</p>
    {:else if conflict.kind === 'name_reserved'}
      <p>
        Another download is still writing a file called
        <strong class="name">{conflict.existing_name}</strong>, so this name cannot be replaced.
      </p>
    {:else}
      <p>
        A file named <strong class="name">{conflict.existing_name}</strong> is already in the download
        folder{existingDetails ? ` (${existingDetails})` : ''}.
      </p>
    {/if}
    <p class="hint">
      Keep both and the new file is saved as <strong class="name">{conflict.suggested_name}</strong>.
      {#if canOverwrite}Or replace the existing file once the new download finishes.{/if}
    </p>
  {/if}

  {#snippet footer()}
    <button class="btn" disabled={busy} onclick={() => onchoose(null)}>Cancel</button>
    {#if canOverwrite}
      <button class="btn danger" disabled={busy} onclick={() => onchoose('overwrite')}>
        Replace existing
      </button>
    {/if}
    <button class="btn primary" disabled={busy} onclick={() => onchoose('rename')}>
      Keep both
    </button>
  {/snippet}
</Modal>

<style>
  .name {
    overflow-wrap: anywhere;
  }
</style>
