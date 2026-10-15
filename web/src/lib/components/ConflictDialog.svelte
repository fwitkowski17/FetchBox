<script lang="ts">
  import type { Conflict, OnConflict } from '../api/types';
  import { i18n, t } from '../i18n';
  import Modal from './Modal.svelte';
  import Trans from './Trans.svelte';

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
    conflict?.kind === 'duplicate_url' ? t('conflict.title.duplicate') : t('conflict.title.name'),
  );

  const existingDetails = $derived.by(() => {
    if (!conflict) return '';
    const parts: string[] = [];
    if (conflict.existing_size != null) parts.push(i18n.bytes(conflict.existing_size));
    if (conflict.existing_modified != null) parts.push(i18n.date(conflict.existing_modified));
    return parts.join(' · ');
  });
</script>

<Modal open={conflict !== null} {title} onclose={() => !busy && onchoose(null)}>
  {#if conflict}
    <p class="[overflow-wrap:anywhere]">
      {#if conflict.kind === 'duplicate_url'}
        <Trans message="conflict.duplicate" params={{ name: conflict.existing_name }} />
      {:else if conflict.kind === 'name_reserved'}
        <Trans message="conflict.reserved" params={{ name: conflict.existing_name }} />
      {:else if existingDetails}
        <Trans
          message="conflict.existsDetails"
          params={{ name: conflict.existing_name, details: existingDetails }}
        />
      {:else}
        <Trans message="conflict.exists" params={{ name: conflict.existing_name }} />
      {/if}
    </p>
    <p class="hint [overflow-wrap:anywhere]">
      <Trans message="conflict.hint.keepBoth" params={{ name: conflict.suggested_name }} />
      {#if canOverwrite}{t('conflict.hint.replace')}{/if}
    </p>
  {/if}

  {#snippet footer()}
    <button class="btn" disabled={busy} onclick={() => onchoose(null)}>{t('common.cancel')}</button>
    {#if canOverwrite}
      <button class="btn danger" disabled={busy} onclick={() => onchoose('overwrite')}>
        {t('conflict.replace')}
      </button>
    {/if}
    <button class="btn primary" disabled={busy} onclick={() => onchoose('rename')}>
      {t('conflict.keepBoth')}
    </button>
  {/snippet}
</Modal>

