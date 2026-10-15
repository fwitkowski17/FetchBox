<script lang="ts">
  // The speed limit of one download. It works on top of the global limit (Settings); the lower of
  // the two counts. Saved limits apply at once, also to a running download.
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
  const global = $derived(settings.value?.speed_global ?? 0);

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
        hint={global > 0
          ? t('speed.hint.global', { speed: i18n.speed(global) })
          : t('speed.hint.noGlobal')}
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
