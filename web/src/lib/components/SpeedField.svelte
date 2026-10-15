<script lang="ts">
  import { t } from '../i18n';
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
        <option value="KB">KB/s</option>
        <option value="MB">MB/s</option>
      </select>
    </div>
  {:else}
    <p class="hint">{offText ?? t('settings.speed.unlimited')}</p>
  {/if}
  {#if error}<p class="text-sm text-danger" role="alert">{error}</p>{/if}
</div>
