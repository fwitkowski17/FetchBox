<script lang="ts">
  // Shown when "Resume" is refused because the disk is still too full: the same free / needs /
  // missing gauge as on the card, big enough to read, following the disk figures while the user
  // frees space. Once there is room the button says "Resume" and works.
  import { t } from '../i18n';
  import { disk } from '../stores/disk.svelte';
  import { downloads } from '../stores/downloads.svelte';
  import { spaceNeed } from '../util/space';
  import Modal from './Modal.svelte';
  import SpaceGauge from './SpaceGauge.svelte';

  const id = $derived(downloads.noSpaceFor);
  const item = $derived(id === null ? undefined : downloads.items.find((d) => d.id === id));
  const open = $derived(item !== undefined && item.size != null);
  const room = $derived(
    item?.size != null
      ? spaceNeed(item.size, item.downloaded, disk.value?.available ?? null).room
      : false,
  );
  let busy = $state(false);

  // The download vanished (removed elsewhere) or started by other means: nothing left to explain.
  $effect(() => {
    if (id !== null && (item === undefined || (item.status !== 'paused' && item.status !== 'failed'))) {
      downloads.noSpaceFor = null;
    }
  });

  function close(): void {
    downloads.noSpaceFor = null;
  }

  async function retry(): Promise<void> {
    if (id === null || busy) return;
    busy = true;
    try {
      await downloads.resume(id); // closes the dialog on success; a new refusal keeps it open
    } finally {
      busy = false;
    }
  }
</script>

<Modal {open} title={t('resumeSpace.title')} onclose={close}>
  {#if item && item.size != null}
    <p class="truncate font-semibold" title={item.filename}>{item.filename}</p>
    <SpaceGauge size={item.size} downloaded={item.downloaded} hint={false} />
    <p class="hint">{t('resumeSpace.text')}</p>
  {/if}

  {#snippet footer()}
    <button class="btn" disabled={busy} onclick={close}>{t('common.dismiss')}</button>
    <button class="btn primary" disabled={busy} onclick={retry}>
      {room ? t('card.resume') : t('common.tryAgain')}
    </button>
  {/snippet}
</Modal>
