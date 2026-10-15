<script lang="ts">
  // "Delete download(s)": removes the entries and, for finished ones, can take the files with them.
  // Unfinished downloads are stopped first and their partial files always go.
  import { i18n, t } from '../i18n';
  import { deleteDialog } from '../stores/deleteDialog.svelte';
  import { downloads } from '../stores/downloads.svelte';
  import { toast } from '../stores/toast.svelte';
  import { bulkInfo, pickSelected } from '../util/bulk';
  import Modal from './Modal.svelte';
  import Trans from './Trans.svelte';

  const targets = $derived(pickSelected(downloads.items, new Set(deleteDialog.ids)));
  const info = $derived(bulkInfo(targets));
  const many = $derived(targets.length > 1);
  const first = $derived(targets[0]);

  let alsoFile = $state(false);
  let busy = $state(false);

  // The downloads are gone (removed elsewhere): nothing left to confirm.
  $effect(() => {
    if (deleteDialog.open && targets.length === 0) deleteDialog.hide();
  });

  // Every opening starts from the safe choice: keep the files.
  $effect(() => {
    if (deleteDialog.open) alsoFile = false;
  });

  function close(): void {
    if (!busy) deleteDialog.hide();
  }

  async function confirm(): Promise<void> {
    if (targets.length === 0 || busy) return;
    busy = true;
    try {
      const ids = targets.map((d) => d.id);
      const result = await downloads.removeMany(ids, info.withFile > 0 && alsoFile);
      if (result.failed > 0) {
        toast.error(`${t('bulk.failed', { n: result.failed })}: ${i18n.errorText(result.firstError)}`);
      }
      if (result.done > 0 && ids.length > 1) toast.success(t('bulk.deleted', { n: result.done }));
      deleteDialog.hide();
    } finally {
      busy = false;
    }
  }
</script>

<Modal open={targets.length > 0} title={many ? t('delete.titleMany') : t('delete.title')} onclose={close}>
  {#if first}
    {#if many}
      <p>{t('delete.message.many', { n: targets.length })}</p>
      {#if info.unfinished > 0}
        <p class="hint">{t('delete.message.manyActive', { n: info.unfinished })}</p>
      {/if}
    {:else}
      <p class="[overflow-wrap:anywhere]">
        <Trans
          message={info.unfinished > 0 ? 'delete.message.active' : 'delete.message.list'}
          params={{ name: first.filename }}
        />
      </p>
    {/if}

    {#if info.withFile > 0}
      <label class="flex items-start gap-3 rounded-lg border border-border p-3" data-part="delete-file">
        <input type="checkbox" class="mt-1 size-4 accent-[var(--danger)]" bind:checked={alsoFile} />
        <span>
          <span class="font-medium">
            {many ? t('delete.alsoFiles', { n: info.withFile }) : t('delete.alsoFile')}
          </span>
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
      {#if many}
        {t(alsoFile && info.withFile > 0 ? 'delete.confirmManyWithFiles' : 'delete.confirmMany', {
          n: targets.length,
        })}
      {:else}
        {t(alsoFile && info.withFile > 0 ? 'delete.confirmWithFile' : 'delete.confirm')}
      {/if}
    </button>
  {/snippet}
</Modal>
