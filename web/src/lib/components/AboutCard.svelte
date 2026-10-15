<script lang="ts">
  // The "About" card of Settings: the running version, whether a newer release exists, and the
  // switch for asking GitHub. The switch belongs to the settings form (`draft`), so it is saved
  // with the rest and shows up in the unsaved-changes bar.
  import { untrack } from 'svelte';
  import { i18n, t } from '../i18n';
  import { settings } from '../stores/settings.svelte';
  import { version } from '../stores/version.svelte';
  import { CARD } from '../util/classes';
  import type { Draft } from '../util/settingsForm';
  import Icon from './Icon.svelte';
  import Toggle from './Toggle.svelte';

  let { draft = $bindable() }: { draft: Draft | null } = $props();

  // Loads on arrival, and again once a change of the switch has been saved.
  $effect(() => {
    void settings.value?.check_updates;
    untrack(() => void version.load());
  });

  const info = $derived(version.info);
  /** Only a web address is offered as a link. */
  const releaseUrl = $derived(info?.release_url?.startsWith('https://') ? info.release_url : null);
</script>

<section class={CARD} data-part="about">
  <h2 class="font-semibold tracking-tight">{t('settings.about')}</h2>

  <div class="grid gap-1">
    <p class="flex flex-wrap items-baseline gap-x-2">
      <span class="font-semibold">FetchBox</span>
      <span class="tabular-nums" data-part="version">{info ? info.current : '…'}</span>
    </p>

    <div aria-live="polite" data-part="update-status" data-state={info === null ? 'loading' : !info.enabled ? 'off' : info.check_failed ? 'failed' : info.update_available ? 'available' : 'current'}>
      {#if info === null}
        <p class="hint">{version.error ? i18n.errorText(version.error) : t('settings.update.checking')}</p>
      {:else if !info.enabled}
        <p class="hint">{t('settings.update.off')}</p>
      {:else if info.update_available && info.latest}
        <p class="flex items-start gap-2 font-semibold text-accent">
          <Icon name="down" size={18} />{t('settings.update.available', { version: info.latest })}
        </p>
      {:else if info.check_failed}
        <p class="hint">{t('settings.update.failed')}</p>
      {:else if info.latest}
        <p class="hint">{t('settings.update.current')}</p>
      {:else}
        <p class="hint">{t('settings.update.none')}</p>
      {/if}
      {#if info?.enabled && info.checked_at}
        <p class="hint">{t('settings.update.checked', { date: i18n.date(info.checked_at) })}</p>
      {/if}
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-2">
    {#if releaseUrl && info?.update_available}
      <a class="btn primary" href={releaseUrl} target="_blank" rel="noopener noreferrer" data-part="release-link">
        {t('settings.update.view')}
      </a>
    {/if}
    <button
      type="button"
      class="btn"
      disabled={version.busy || !info?.enabled}
      onclick={() => version.load(true)}
      data-part="check-now"
    >
      <Icon name="retry" size={18} />{version.busy ? t('settings.update.checkingShort') : t('settings.update.check')}
    </button>
  </div>

  {#if draft}
    <Toggle bind:checked={draft.checkUpdates} hint={t('settings.update.hint')}>
      {t('settings.update.toggle')}
    </Toggle>
  {/if}
</section>
