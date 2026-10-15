<script lang="ts">
  // "Delete download": removes the entry and, for a finished download, can take the file with it.
  // An unfinished one is stopped first and its partial file always goes.
  import { t } from '../i18n';
  import { deleteDialog } from '../stores/deleteDialog.svelte';
  import { downloads } from '../stores/downloads.svelte';
  import Modal from './Modal.svelte';
  import Trans from './Trans.svelte';

  const item = $derived(
    deleteDialog.id === null ? undefined : downloads.items.find((d) => d.id === deleteDialog.id),
  );
  const finished = $derived(item?.status === 'completed');
  const unfinished = $derived(
    item !== undefined && ['queued', 'downloading', 'paused'].includes(item.status),
  );

  let alsoFile = $state(false);
  let busy = $state(false);

  // The download is gone (removed elsewhere): nothing left to confirm.
  $effect(() => {
    if (deleteDialog.id !== null && item === undefined) deleteDialog.hide();
  });

  // Every opening starts from the safe choice: keep the file.
  $effect(() => {
    if (deleteDialog.id !== null) alsoFile = false;
  });

  function close(): void {
    if (!busy) deleteDialog.hide();
  }

  async function confirm(): Promise<void> {
    if (!item || busy) return;
    busy = true;
    try {
      const ok = await downloads.remove(item.id, finished && alsoFile);
      if (ok) deleteDialog.hide();
    } finally {
      busy = false;
    }
  }
</script>

<Modal open={item !== undefined} title={t('delete.title')} onclose={close}>
  {#if item}
    <p class="[overflow-wrap:anywhere]">
      <Trans
        message={unfinished ? 'delete.message.active' : 'delete.message.list'}
        params={{ name: item.filename }}
      />
    </p>

    {#if finished}
      <label class="flex items-start gap-3 rounded-lg border border-border p-3" data-part="delete-file">
        <input type="checkbox" class="mt-1 size-4 accent-[var(--danger)]" bind:checked={alsoFile} />
        <span>
          <span class="font-medium">{t('delete.alsoFile')}</span>
          <span class={['mt-0.5 block text-sm', alsoFile ? 'text-danger' : 'text-muted']}>
            {alsoFile ? t('delete.alsoFile.warning') : t('delete.alsoFile.hint')}
          </span>
        </span>
      </label>
    {/if}
  {/if}

  {#snippet footer()}
    <button class="btn" disabled={busy} onclick={close}>{t('common.cancel')}</button>
    <button class="btn danger solid" disabled={busy} onclick={confirm}>
      {t(finished && alsoFile ? 'delete.confirmWithFile' : 'delete.confirm')}
    </button>
  {/snippet}
</Modal>
