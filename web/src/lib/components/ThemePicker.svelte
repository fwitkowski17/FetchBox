<script lang="ts">
  import { t } from '../i18n';
  import { theme, type ThemePref } from '../stores/theme.svelte';
  import Icon, { type IconName } from './Icon.svelte';

  const OPTIONS: readonly { id: ThemePref; icon: IconName }[] = [
    { id: 'system', icon: 'monitor' },
    { id: 'light', icon: 'sun' },
    { id: 'dark', icon: 'moon' },
    { id: 'black', icon: 'contrast' },
  ];
</script>

<div
  class="grid grid-cols-[repeat(auto-fit,minmax(6.5rem,1fr))] gap-1 rounded-lg border border-border bg-surface-2 p-1"
  role="radiogroup"
  aria-label={t('settings.theme')}
>
  {#each OPTIONS as o (o.id)}
    <label
      class="relative flex min-h-10 cursor-pointer items-center justify-center gap-2 rounded-md font-semibold text-muted transition-colors has-checked:bg-surface has-checked:text-text has-checked:shadow-card has-focus-visible:outline-2 has-focus-visible:outline-accent"
    >
      <input
        type="radio"
        name="theme"
        value={o.id}
        class="absolute inset-0 cursor-pointer opacity-0"
        checked={theme.pref === o.id}
        onchange={() => theme.set(o.id)}
      />
      <Icon name={o.icon} size={18} />
      <span>{t(`theme.${o.id}`)}</span>
    </label>
  {/each}
</div>
