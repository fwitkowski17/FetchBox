<script lang="ts">
  // A ring with the used share of the download disk, a lighter arc for what unfinished downloads
  // will still write, and the numbers next to it. Amber from 85 %, red from 95 % or when the
  // queue does not fit.
  import { i18n, t } from '../i18n';
  import { untrack } from 'svelte';
  import { disk } from '../stores/disk.svelte';
  import { disks } from '../stores/disks.svelte';
  import { settings } from '../stores/settings.svelte';
  import { diskView } from '../util/disk';
  import { formatPercent } from '../util/format';

  const STROKE = {
    ok: 'stroke-accent',
    warn: 'stroke-warning',
    full: 'stroke-danger',
  } as const;
  /** The arcs glide to new values instead of jumping when the figures are refreshed. */
  const RING_MOTION = 'transition-[stroke-dasharray,stroke-dashoffset,stroke] duration-700 ease-out';
  const TEXT = { ok: 'text-muted', warn: 'text-warning', full: 'text-danger' } as const;

  const value = $derived(disk.value);

  // The ring shows the disk new downloads go to, so it follows a change of the default.
  $effect(() => {
    void settings.value?.default_disk;
    untrack(() => {
      void disk.refresh();
      void disks.load();
    });
  });
  /** Which disk it is, when there is more than one to tell apart. */
  const diskName = $derived.by(() => {
    const found = disks.value;
    if (!found?.choosable) return null;
    return found.default === '' ? t('disks.main') : found.default;
  });
  const view = $derived(value ? diskView(value) : null);
  // Percent of the circle, for `pathLength="100"`.
  const usedLen = $derived(view ? view.used * 100 : 0);
  const reservedLen = $derived(view ? view.reserved * 100 : 0);

  const summary = $derived(
    value
      ? t('disk.summary', {
          free: i18n.bytes(value.available),
          total: i18n.bytes(value.total),
          percent: formatPercent(view?.used ?? 0),
        })
      : '',
  );
</script>

{#if value && view}
  <div class="flex items-center gap-3 px-3 py-1" data-part="disk" data-level={view.level} title={summary}>
    <div class="relative size-12 shrink-0">
      <svg viewBox="0 0 36 36" class="size-full -rotate-90" role="img" aria-label={summary}>
        <circle cx="18" cy="18" r="15.9155" fill="none" class="stroke-border" stroke-width="4" />
        {#if reservedLen > 0}
          <!-- Starts where the used arc ends -->
          <circle
            cx="18"
            cy="18"
            r="15.9155"
            fill="none"
            class={[STROKE[view.level], 'opacity-35', RING_MOTION]}
            stroke-width="4"
            pathLength="100"
            stroke-dasharray="{reservedLen} {100 - reservedLen}"
            stroke-dashoffset={-usedLen}
            data-part="disk-reserved"
          />
        {/if}
        <circle
          cx="18"
          cy="18"
          r="15.9155"
          fill="none"
          class={[STROKE[view.level], RING_MOTION]}
          stroke-width="4"
          stroke-linecap="butt"
          pathLength="100"
          stroke-dasharray="{usedLen} {100 - usedLen}"
          data-part="disk-used"
        />
      </svg>
      <span
        class="absolute inset-0 grid place-items-center text-[0.7rem] font-semibold tabular-nums"
        aria-hidden="true"
      >
        {formatPercent(view.used)}
      </span>
    </div>

    <div class="min-w-0 text-sm leading-tight">
      <p class="font-semibold text-text">{t('disk.free', { size: i18n.bytes(value.available) })}</p>
      <p class="text-muted">{t('disk.of', { size: i18n.bytes(value.total) })}</p>
      {#if diskName}
        <p class="truncate font-medium text-muted" title={diskName} data-part="disk-name">{diskName}</p>
      {/if}
      {#if view.short > 0}
        <p class={['mt-0.5 font-medium', TEXT.full]} role="status" data-part="disk-short">
          {t('disk.short', { size: i18n.bytes(view.short) })}
        </p>
      {:else if value.reserved > 0}
        <p class="mt-0.5 text-muted" data-part="disk-reserved-text">
          {t('disk.reserved', { size: i18n.bytes(value.reserved) })}
        </p>
      {/if}
    </div>
  </div>
{/if}
