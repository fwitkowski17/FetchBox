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
  import DefaultDiskCard from './DefaultDiskCard.svelte';
  import HoursFields from './HoursFields.svelte';
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

  <section class={CARD} data-part="torrent-limits">
    <h2 class="font-semibold tracking-tight">{t('settings.torrents.title')}</h2>
    <p class="hint">{t('settings.torrents.hint')}</p>
    <SpeedField
      label={t('settings.torrents.down')}
      hint=""
      bind:value={draft.torrentDown}
      error={errors.torrentDown ? errorText(errors.torrentDown) : undefined}
    />
    <hr class="m-0 w-full border-0 border-t border-border" />
    <SpeedField
      label={t('settings.torrents.up')}
      hint=""
      bind:value={draft.torrentUp}
      error={errors.torrentUp ? errorText(errors.torrentUp) : undefined}
    />
  </section>

  <section class={CARD} data-part="schedule">
    <h2 class="font-semibold tracking-tight">{t('settings.schedule')}</h2>
    <Toggle bind:checked={draft.scheduleEnabled} hint={t('settings.schedule.hint')}>
      {t('settings.schedule.toggle')}
    </Toggle>
    {#if draft.scheduleEnabled}
      <HoursFields
        id="schedule"
        bind:from={draft.scheduleFrom}
        bind:to={draft.scheduleTo}
        bind:timezone={draft.scheduleTimezone}
        scheduleError={errors.schedule ? errorText(errors.schedule) : undefined}
        timezoneError={errors.timezone ? errorText(errors.timezone) : undefined}
        summaryKey="settings.schedule.summary"
        summaryOvernightKey="settings.schedule.summaryOvernight"
      >
        {#snippet middle()}
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
        {/snippet}
      </HoursFields>
    {/if}
  </section>

  <section class={CARD} data-part="torrent-schedule">
    <h2 class="font-semibold tracking-tight">{t('settings.torrentSchedule')}</h2>
    <Toggle bind:checked={draft.torrentScheduleEnabled} hint={t('settings.torrentSchedule.hint')}>
      {t('settings.torrentSchedule.toggle')}
    </Toggle>
    {#if draft.torrentScheduleEnabled}
      <HoursFields
        id="torrent-schedule"
        bind:from={draft.torrentScheduleFrom}
        bind:to={draft.torrentScheduleTo}
        bind:timezone={draft.torrentScheduleTimezone}
        scheduleError={errors.torrentSchedule ? errorText(errors.torrentSchedule) : undefined}
        timezoneError={errors.torrentTimezone ? errorText(errors.torrentTimezone) : undefined}
        summaryKey="settings.torrentSchedule.summary"
        summaryOvernightKey="settings.torrentSchedule.summaryOvernight"
      />
      <p class="hint" data-part="torrent-schedule-note">{t('settings.torrentSchedule.note')}</p>
    {/if}
  </section>

  <DefaultDiskCard bind:draft />

  <section class={CARD}>
    <h2 class="font-semibold tracking-tight">{t('settings.files')}</h2>
    <Toggle bind:checked={draft.categorySubdirs} hint={t('settings.subdirs.hint')}>
      {t('settings.subdirs')}
    </Toggle>
  </section>
