<script lang="ts">
  // The fields of a set of hours (from, to, time zone), shared by the download hours and the
  // torrent hours. What belongs to only one of them (what happens when the hours end) goes in
  // `middle`, between the summary and the time zone.
  import type { Snippet } from 'svelte';
  import { t } from '../i18n';
  import { knownZones, timeToMinutes } from '../util/schedule';

  let {
    id,
    from = $bindable(),
    to = $bindable(),
    timezone = $bindable(),
    scheduleError,
    timezoneError,
    summaryKey,
    summaryOvernightKey,
    middle,
  }: {
    /** Prefix of the element ids (`schedule` gives `schedule-from`, ...). */
    id: string;
    from: string;
    to: string;
    timezone: string;
    scheduleError?: string;
    timezoneError?: string;
    summaryKey: string;
    summaryOvernightKey: string;
    middle?: Snippet;
  } = $props();

  const zones = knownZones();
  const overnight = $derived.by(() => {
    const a = timeToMinutes(from);
    const b = timeToMinutes(to);
    return a !== null && b !== null && b < a;
  });
</script>

<div class="grid gap-4 sm:grid-cols-2">
  <div class="field">
    <label for="{id}-from">{t('settings.schedule.from')}</label>
    <input
      id="{id}-from"
      class="input"
      type="time"
      required
      bind:value={from}
      aria-invalid={scheduleError ? 'true' : undefined}
    />
  </div>
  <div class="field">
    <label for="{id}-to">{t('settings.schedule.to')}</label>
    <input
      id="{id}-to"
      class="input"
      type="time"
      required
      bind:value={to}
      aria-invalid={scheduleError ? 'true' : undefined}
    />
  </div>
</div>
{#if scheduleError}
  <p class="text-sm text-danger" role="alert">{scheduleError}</p>
{:else}
  <p class="hint" data-part="{id}-summary">
    {t((overnight ? summaryOvernightKey : summaryKey) as 'settings.schedule.summary', { from, to })}
  </p>
{/if}
{@render middle?.()}
<div class="field">
  <label for="{id}-zone">{t('settings.schedule.zone')}</label>
  <input
    id="{id}-zone"
    class="input"
    list="schedule-zones-{id}"
    autocomplete="off"
    autocapitalize="off"
    spellcheck="false"
    bind:value={timezone}
    aria-invalid={timezoneError ? 'true' : undefined}
  />
  <p class="hint">{t('settings.schedule.zoneHint')}</p>
  {#if timezoneError}
    <p class="text-sm text-danger" role="alert">{timezoneError}</p>
  {/if}
</div>
<datalist id="schedule-zones-{id}">
  {#each zones as zone (zone)}<option value={zone}></option>{/each}
</datalist>
