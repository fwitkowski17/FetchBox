<script lang="ts">
  // The speed of one download. A limit of its own replaces the default per-download speed of the
  // settings (so the download may be faster or slower than the others); the total limit of all
  // downloads still caps the sum. Saved limits apply at once, also to a running download.
  import { i18n, t } from '../i18n';
  import { downloads } from '../stores/downloads.svelte';
  import { settings } from '../stores/settings.svelte';
  import { speedDialog } from '../stores/speedDialog.svelte';
  import {
    isValidSpeed,
    speedFromDraft,
    speedToDraft,
    type SpeedDraft,
  } from '../util/settingsForm';
  import Modal from './Modal.svelte';
  import SpeedField from './SpeedField.svelte';

  const item = $derived(
    speedDialog.id === null ? undefined : downloads.items.find((d) => d.id === speedDialog.id),
  );
  const defaultRate = $derived(settings.value?.speed_per_download ?? 0);

  let draft = $state<SpeedDraft>({ limited: false, value: 1, unit: 'MB' });
  let busy = $state(false);
  const valid = $derived(isValidSpeed(draft));
  const unchanged = $derived(item !== undefined && speedFromDraft(draft) === (item.speed_limit ?? 0));

  // Every opening starts from what the download has now.
  $effect(() => {
    if (speedDialog.id !== null) draft = speedToDraft(item?.speed_limit ?? 0);
  });

  // The download is gone, or finished meanwhile: nothing left to limit.
  $effect(() => {
    if (speedDialog.id !== null && (item === undefined || ['completed', 'cancelled'].includes(item.status))) {
      speedDialog.hide();
    }
  });

  function close(): void {
    if (!busy) speedDialog.hide();
  }

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (!item || !valid || busy) return;
    busy = true;
    try {
      if (await downloads.setSpeedLimit(item.id, speedFromDraft(draft))) speedDialog.hide();
    } finally {
      busy = false;
    }
  }
</script>

<Modal open={item !== undefined} title={t('speed.title')} onclose={close}>
  {#if item}
    <p class="truncate font-semibold" title={item.filename}>{item.filename}</p>
    <form id="speed-form" class="grid gap-3" onsubmit={save}>
      <SpeedField
        label={t('speed.limitThis')}
        hint={t('speed.hint')}
        offText={defaultRate > 0
          ? t('speed.off.default', { speed: i18n.speed(defaultRate) })
          : t('speed.off.none')}
        bind:value={draft}
        error={valid ? undefined : t('settings.error.speed')}
      />
    </form>
  {/if}

  {#snippet footer()}
    <button class="btn" type="button" disabled={busy} onclick={close}>{t('common.cancel')}</button>
    <button class="btn primary" type="submit" form="speed-form" disabled={!valid || unchanged || busy}>
      {busy ? t('settings.saving') : t('settings.save')}
    </button>
  {/snippet}
</Modal>
