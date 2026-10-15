<script lang="ts">
  import { t } from '../i18n';
  import { theme, type ThemePref } from '../stores/theme.svelte';
  import Icon, { type IconName } from './Icon.svelte';

  const OPTIONS: readonly { id: ThemePref; icon: IconName }[] = [
    { id: 'system', icon: 'monitor' },
    { id: 'light', icon: 'sun' },
    { id: 'dark', icon: 'moon' },
  ];
</script>

<div class="seg" role="radiogroup" aria-label={t('settings.theme')}>
  {#each OPTIONS as o (o.id)}
    <label class:on={theme.pref === o.id}>
      <input
        type="radio"
        name="theme"
        value={o.id}
        checked={theme.pref === o.id}
        onchange={() => theme.set(o.id)}
      />
      <Icon name={o.icon} size={18} />
      <span>{t(`theme.${o.id}`)}</span>
    </label>
  {/each}
</div>

<style>
  .seg {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.25rem;
    padding: 0.25rem;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--border);
  }

  label {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.45rem;
    min-height: 2.5rem;
    border-radius: 7px;
    font-weight: 600;
    color: var(--muted);
    cursor: pointer;
  }

  label.on {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  input {
    position: absolute;
    opacity: 0;
    inset: 0;
    cursor: pointer;
  }

  label:has(input:focus-visible) {
    outline: 2px solid var(--accent);
  }
</style>
