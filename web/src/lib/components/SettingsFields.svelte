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
  import { knownZones, timeToMinutes } from '../util/schedule';
  import SpeedField from './SpeedField.svelte';
  import Toggle from './Toggle.svelte';

  let { draft = $bindable(), errors }: { draft: Draft; errors: Errors } = $props();


  const zones = knownZones();
  const overnight = $derived.by(() => {
    const from = timeToMinutes(draft.scheduleFrom);
    const to = timeToMinutes(draft.scheduleTo);
    return from !== null && to !== null && to < from;
  });

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

  <section class={CARD} data-part="schedule">
    <h2 class="font-semibold tracking-tight">{t('settings.schedule')}</h2>
    <Toggle bind:checked={draft.scheduleEnabled} hint={t('settings.schedule.hint')}>
      {t('settings.schedule.toggle')}
    </Toggle>
    {#if draft.scheduleEnabled}
      <div class="grid gap-4 sm:grid-cols-2">
        <div class="field">
          <label for="schedule-from">{t('settings.schedule.from')}</label>
          <input
            id="schedule-from"
            class="input"
            type="time"
            required
            bind:value={draft.scheduleFrom}
            aria-invalid={errors.schedule ? 'true' : undefined}
          />
        </div>
        <div class="field">
          <label for="schedule-to">{t('settings.schedule.to')}</label>
          <input
            id="schedule-to"
            class="input"
            type="time"
            required
            bind:value={draft.scheduleTo}
            aria-invalid={errors.schedule ? 'true' : undefined}
          />
        </div>
      </div>
      {#if errors.schedule}
        <p class="text-sm text-danger" role="alert">{errorText(errors.schedule)}</p>
      {:else}
        <p class="hint" data-part="schedule-summary">
          {t(overnight ? 'settings.schedule.summaryOvernight' : 'settings.schedule.summary', {
            from: draft.scheduleFrom,
            to: draft.scheduleTo,
          })}
        </p>
      {/if}
      <fieldset class="field m-0 border-0 p-0" data-part="on-end">
        <legend class="mb-1 font-semibold">{t('settings.schedule.onEnd')}</legend>
        <label class="flex cursor-pointer items-start gap-3">
          <input
            type="radio"
            name="schedule-on-end"
            value="pause"
            class="mt-1 size-5 shrink-0 accent-[var(--accent)]"
            bind:group={draft.scheduleOnEnd}
          />
          <span class="grid gap-0.5">
            <span class="font-medium">{t('settings.schedule.onEnd.pause')}</span>
            <span class="hint">{t('settings.schedule.onEnd.pauseHint')}</span>
          </span>
        </label>
        <label class="flex cursor-pointer items-start gap-3">
          <input
            type="radio"
            name="schedule-on-end"
            value="finish"
            class="mt-1 size-5 shrink-0 accent-[var(--accent)]"
            bind:group={draft.scheduleOnEnd}
          />
          <span class="grid gap-0.5">
            <span class="font-medium">{t('settings.schedule.onEnd.finish')}</span>
            <span class="hint">{t('settings.schedule.onEnd.finishHint')}</span>
          </span>
        </label>
      </fieldset>

      <div class="field">
        <label for="schedule-zone">{t('settings.schedule.zone')}</label>
        <input
          id="schedule-zone"
          class="input"
          list="schedule-zones"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          bind:value={draft.scheduleTimezone}
          aria-invalid={errors.timezone ? 'true' : undefined}
        />
        <datalist id="schedule-zones">
          {#each zones as zone (zone)}<option value={zone}></option>{/each}
        </datalist>
        <p class="hint">{t('settings.schedule.zoneHint')}</p>
        {#if errors.timezone}
          <p class="text-sm text-danger" role="alert">{errorText(errors.timezone)}</p>
        {/if}
      </div>
    {/if}
  </section>

  <section class={CARD}>
    <h2 class="font-semibold tracking-tight">{t('settings.files')}</h2>
    <Toggle bind:checked={draft.categorySubdirs} hint={t('settings.subdirs.hint')}>
      {t('settings.subdirs')}
    </Toggle>
  </section>
