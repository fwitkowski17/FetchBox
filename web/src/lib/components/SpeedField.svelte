<script lang="ts">
  import { t } from '../i18n';
  import type { SpeedDraft } from '../util/settingsForm';
  import Toggle from './Toggle.svelte';

  interface Props {
    label: string;
    hint: string;
    value: SpeedDraft;
    error?: string | undefined;
  }

  let { label, hint, value = $bindable(), error }: Props = $props();
  const id = `speed-${Math.random().toString(36).slice(2, 8)}`;
</script>

<div class="speed">
  <Toggle bind:checked={value.limited} {hint}>{label}</Toggle>
  {#if value.limited}
    <div class="row">
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
      <select id={`${id}-unit`} class="select unit" bind:value={value.unit}>
        <option value="KB">KB/s</option>
        <option value="MB">MB/s</option>
      </select>
    </div>
  {:else}
    <p class="unlimited">{t('settings.speed.unlimited')}</p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .speed {
    display: grid;
    gap: 0.5rem;
  }

  .row {
    display: flex;
    gap: 0.5rem;
  }

  .unit {
    flex: 0 0 6.5rem;
    width: 6.5rem;
  }

  .unlimited {
    color: var(--muted);
    font-size: 0.9rem;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }
</style>
