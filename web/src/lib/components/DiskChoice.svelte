<script lang="ts">
  // The disks to choose from, as cards: name, where it is, and how much room it has. Used by the
  // first-run step and by Settings. Selecting one only changes `value`; saving is the caller's.
  import type { DiskInfo } from '../api/types';
  import { i18n, t } from '../i18n';
  import { FULL_AT, WARN_AT } from '../util/disk';
  import { usedShare, warningKey } from '../util/disks';
  import Icon from './Icon.svelte';

  let {
    disks,
    value = $bindable(),
    group,
    missing = null,
  }: {
    disks: readonly DiskInfo[];
    /** The chosen disk's name (empty = the download directory itself); `null` = none chosen yet. */
    value: string | null;
    /** `name` of the radio group. */
    group: string;
    /** A stored choice that is not mounted now: shown, but it cannot be picked. */
    missing?: string | null;
  } = $props();

  function level(d: DiskInfo): 'ok' | 'warn' | 'full' {
    const used = usedShare(d);
    return used >= FULL_AT ? 'full' : used >= WARN_AT ? 'warn' : 'ok';
  }
  const FILL = { ok: 'bg-accent', warn: 'bg-warning', full: 'bg-danger' } as const;
</script>

<div class="grid gap-2" role="radiogroup" aria-label={t('disks.choose')} data-part="disk-choice">
  {#each disks as d (d.name)}
    <label
      class={[
        'grid cursor-pointer gap-2 rounded-xl border p-3 transition-colors hover:border-accent/60',
        value === d.name ? 'border-accent bg-accent-soft' : 'border-border bg-surface',
      ]}
      data-disk={d.name}
    >
      <span class="flex items-start gap-3">
        <input
          type="radio"
          name={group}
          class="mt-1 size-5 shrink-0 accent-[var(--accent)]"
          value={d.name}
          checked={value === d.name}
          onchange={() => (value = d.name)}
        />
        <span class="grid min-w-0 flex-1 gap-0.5">
          <span class="flex flex-wrap items-baseline justify-between gap-x-3">
            <span class="font-semibold">{d.name === '' ? t('disks.main') : d.name}</span>
            <span class="text-sm tabular-nums text-muted">
              {t('disks.capacity', { free: i18n.bytes(d.available), total: i18n.bytes(d.total) })}
            </span>
          </span>
          <span class="truncate font-mono text-xs text-muted" title={d.path}>{d.path}</span>
        </span>
      </span>
      {#if d.warning}
        <span class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" data-part="disk-warning" data-warning={d.warning}>
          <Icon name="alert" size={18} />{t(warningKey(d.warning))}
        </span>
      {/if}
      <span
        class="block h-1.5 overflow-hidden rounded-full bg-border"
        role="img"
        aria-label={t('disks.used', { percent: Math.round(usedShare(d) * 100) })}
      >
        <span class={['block h-full rounded-full', FILL[level(d)]]} style:width="{usedShare(d) * 100}%"></span>
      </span>
    </label>
  {/each}
  {#if missing}
    <p class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" role="status" data-part="disk-missing">
      <Icon name="alert" size={18} />{t('disks.missing', { name: missing })}
    </p>
  {/if}
</div>
