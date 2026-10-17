<script lang="ts">
  // "Remove torrent": takes it off the list and stops it; the downloaded files stay unless asked.
  import { t } from '../i18n';
  import { deleteTorrentDialog } from '../stores/deleteTorrentDialog.svelte';
  import { torrents } from '../stores/torrents.svelte';
  import Modal from './Modal.svelte';
  import Trans from './Trans.svelte';

  const torrent = $derived(torrents.items.find((x) => x.id === deleteTorrentDialog.id) ?? null);

  let alsoFiles = $state(false);
  let busy = $state(false);

  // The torrent is gone (removed elsewhere): nothing left to confirm.
  $effect(() => {
    if (deleteTorrentDialog.open && !torrent) deleteTorrentDialog.hide();
  });

  // Every opening starts from the safe choice: keep the files.
  $effect(() => {
    if (deleteTorrentDialog.open) alsoFiles = false;
  });

  function close(): void {
    if (!busy) deleteTorrentDialog.hide();
  }

  async function confirm(): Promise<void> {
    if (!torrent || busy) return;
    busy = true;
    try {
      await torrents.remove(torrent.id, alsoFiles);
      deleteTorrentDialog.hide();
    } finally {
      busy = false;
    }
  }
</script>

<Modal open={torrent !== null} title={t('torrent.delete.title')} onclose={close}>
  {#if torrent}
    <p class="[overflow-wrap:anywhere]">
      <Trans message="torrent.delete.message" params={{ name: torrent.name }} />
    </p>
    <label class="flex items-start gap-3 rounded-lg border border-border p-3" data-part="delete-files">
      <input type="checkbox" class="mt-1 size-4 accent-[var(--danger)]" bind:checked={alsoFiles} />
      <span>
        <span class="font-medium">{t('torrent.delete.alsoFiles')}</span>
        <span class={['mt-0.5 block text-sm', alsoFiles ? 'text-danger' : 'text-muted']}>
          {alsoFiles ? t('torrent.delete.alsoFiles.warning') : t('torrent.delete.alsoFiles.hint')}
        </span>
      </span>
    </label>
  {/if}

  {#snippet footer()}
    <button class="btn" disabled={busy} onclick={close}>{t('common.cancel')}</button>
    <button class="btn danger solid" disabled={busy} onclick={confirm}>
      {t(alsoFiles ? 'torrent.delete.confirmWithFiles' : 'torrent.delete.confirm')}
    </button>
  {/snippet}
</Modal>
