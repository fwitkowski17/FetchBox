<script lang="ts">
  // The download-related settings cards, shared by the Settings page and the first-run setup.
  import { t } from '../i18n';
  import { CARD } from '../util/classes';
  import {
    MAX_CONCURRENT,
    MAX_CONNECTIONS,
    type Draft,
    type ErrorCode,
    type Errors,
  } from '../util/settingsForm';
  import SpeedField from './SpeedField.svelte';
  import Toggle from './Toggle.svelte';

  let { draft = $bindable(), errors }: { draft: Draft; errors: Errors } = $props();


  /** The message for a validation code; the bounds are the form's own. */
  function errorText(code: ErrorCode): string {
    const max = code === 'connections' ? MAX_CONNECTIONS : MAX_CONCURRENT;
    return t(`settings.error.${code}`, { max });
  }
</script>

  <section class={CARD}>
    <h2 class="font-semibold tracking-tight">{t('settings.downloads')}</h2>
    <div class="field">
      <label for="connections">{t('settings.connections.label')}</label>
      <input
        id="connections"
        class="input"
        type="number"
        inputmode="numeric"
        min="1"
        max={MAX_CONNECTIONS}
        bind:value={draft.connections}
        aria-invalid={errors.connections ? 'true' : undefined}
      />
      <p class="hint">{t('settings.connections.hint')}</p>
      {#if errors.connections}
        <p class="text-sm text-danger" role="alert">{errorText(errors.connections)}</p>
      {/if}
    </div>

    <div class="field">
      <label for="concurrent">{t('settings.concurrent.label')}</label>
      <input
        id="concurrent"
        class="input"
        type="number"
        inputmode="numeric"
        min="1"
        max={MAX_CONCURRENT}
        bind:value={draft.maxConcurrent}
        aria-invalid={errors.maxConcurrent ? 'true' : undefined}
      />
      <p class="hint">{t('settings.concurrent.hint')}</p>
      {#if errors.maxConcurrent}
        <p class="text-sm text-danger" role="alert">{errorText(errors.maxConcurrent)}</p>
      {/if}
    </div>
  </section>

  <section class={CARD}>
    <h2 class="font-semibold tracking-tight">{t('settings.speed')}</h2>
    <SpeedField
      label={t('settings.speed.each')}
      hint={t('settings.speed.eachHint')}
      bind:value={draft.perDownload}
      error={errors.perDownload ? errorText(errors.perDownload) : undefined}
    />
    <hr class="m-0 w-full border-0 border-t border-border" />
    <SpeedField
      label={t('settings.speed.total')}
      hint={t('settings.speed.totalHint')}
      bind:value={draft.global}
      error={errors.global ? errorText(errors.global) : undefined}
    />
  </section>

  <section class={CARD}>
    <h2 class="font-semibold tracking-tight">{t('settings.files')}</h2>
    <Toggle bind:checked={draft.categorySubdirs} hint={t('settings.subdirs.hint')}>
      {t('settings.subdirs')}
    </Toggle>
  </section>
