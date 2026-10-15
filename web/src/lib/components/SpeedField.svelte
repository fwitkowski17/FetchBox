<script lang="ts">
  import { t } from '../i18n';
  import { SPEED_UNITS, SPEED_UNIT_LABEL } from '../util/format';
  import type { SpeedDraft } from '../util/settingsForm';
  import Toggle from './Toggle.svelte';

  interface Props {
    label: string;
    hint: string;
    value: SpeedDraft;
    error?: string | undefined;
    /** What the field says while the limit is off (default: "Unlimited"). */
    offText?: string | undefined;
  }

  let { label, hint, value = $bindable(), error, offText }: Props = $props();
  const id = `speed-${Math.random().toString(36).slice(2, 8)}`;
</script>

<div class="grid gap-2">
  <Toggle bind:checked={value.limited} {hint}>{label}</Toggle>
  {#if value.limited}
    <div class="flex gap-2">
      <label class="sr-only" for={id}>{t('settings.speed.max', { label })}</label>
      <input
        {id}
        class="input"
        type="number"
        inputmode="decimal"
        min="0"
        step="any"
        bind:value={value.value}
        aria-invalid={error ? 'true' : undefined}
      />
      <label class="sr-only" for={`${id}-unit`}>{t('settings.speed.unit')}</label>
      <select id={`${id}-unit`} class="select w-28 shrink-0" bind:value={value.unit}>
        {#each SPEED_UNITS as unit (unit)}
          <option value={unit}>{SPEED_UNIT_LABEL[unit]}</option>
        {/each}
      </select>
    </div>
  {:else}
    <p class="hint">{offText ?? t('settings.speed.unlimited')}</p>
  {/if}
  {#if error}<p class="text-sm text-danger" role="alert">{error}</p>{/if}
</div>
