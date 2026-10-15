<script lang="ts">
  // What has been downloaded so far: the totals of a period, the speeds, and the last days.
  // The numbers come from the server's history, so they outlive the entries of the list.
  import { onMount } from 'svelte';
  import CategoryBars from '../lib/components/CategoryBars.svelte';
  import DayChart from '../lib/components/DayChart.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import StatTile from '../lib/components/StatTile.svelte';
  import { i18n, t } from '../lib/i18n';
  import { downloads } from '../lib/stores/downloads.svelte';
  import { RANGES, stats } from '../lib/stores/stats.svelte';
  import { CARD } from '../lib/util/classes';
  import { successPercent } from '../lib/util/stats';

  const POLL_MS = 10_000;

  const s = $derived(stats.value);
  const ended = $derived(s ? s.completed + s.failed + s.cancelled : 0);
  const nothingYet = $derived(s !== null && ended + s.in_progress === 0 && s.bytes === 0);
  const percent = $derived(s ? successPercent(s.completed, s.failed) : null);

  // Fresh numbers while the page is open: on a timer (not in a hidden tab), when the tab comes
  // back, and as soon as a download ends (the list is live, so that is visible there at once).
  const endedKey = $derived(
    downloads.items.filter((d) => d.status === 'completed' || d.status === 'failed' || d.status === 'cancelled')
      .length,
  );
  $effect(() => {
    void endedKey;
    void stats.load();
  });

  onMount(() => {
    const timer = setInterval(() => {
      if (!document.hidden) void stats.load();
    }, POLL_MS);
    const onVisible = (): void => {
      if (!document.hidden) void stats.load();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisible);
    };
  });

  const speed = (v: number): string => (v > 0 ? i18n.speed(v) : '—');
</script>

<div class="grid max-w-4xl gap-4" data-part="stats-page">
  <header class="flex flex-wrap items-center justify-between gap-3">
    <h1 class="text-2xl font-bold tracking-tight">{t('stats.title')}</h1>
    <div
      class="grid grid-cols-4 gap-1 rounded-lg border border-border bg-surface-2 p-1"
      role="radiogroup"
      aria-label={t('stats.range')}
    >
      {#each RANGES as r (r)}
        <label
          class="relative flex min-h-9 cursor-pointer items-center justify-center rounded-md px-3 text-sm font-semibold whitespace-nowrap text-muted transition-colors has-checked:bg-surface has-checked:text-text has-checked:shadow-card has-focus-visible:outline-2 has-focus-visible:outline-accent"
        >
          <input
            type="radio"
            name="stats-range"
            value={r}
            class="absolute inset-0 cursor-pointer opacity-0"
            checked={stats.range === r}
            onchange={() => stats.setRange(r)}
          />
          {t(`stats.range.${r}`)}
        </label>
      {/each}
    </div>
  </header>

  {#if stats.error && !s}
    <div class={CARD} role="alert">
      <p>{t('stats.loadFailed', { error: i18n.errorText(stats.error) })}</p>
      <button class="btn justify-self-start" type="button" onclick={() => stats.load()}>
        {t('common.tryAgain')}
      </button>
    </div>
  {:else if !s}
    <p class="hint" role="status">{t('common.loading')}</p>
  {:else if nothingYet}
    <div class={[CARD, 'justify-items-center py-10 text-center']} data-part="stats-empty">
      <span class="grid size-12 place-items-center rounded-full bg-surface-2 text-muted">
        <Icon name="chart" size={24} />
      </span>
      <h2 class="text-lg font-semibold tracking-tight">{t('stats.empty.title')}</h2>
      <p class="hint max-w-sm">{t('stats.empty.hint')}</p>
    </div>
  {:else}
    <!-- The headline: how much came in. -->
    <section class={CARD} aria-labelledby="stats-data">
      <p id="stats-data" class="text-sm font-medium text-muted">{t('stats.data')}</p>
      <p class="text-5xl font-semibold tracking-tight" data-part="stats-hero">{i18n.bytes(s.bytes)}</p>
      <p class="text-sm text-muted">{t('stats.data.from', { n: ended })}</p>
    </section>

    <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
      <StatTile
        label={t('stats.downloads')}
        value={String(ended)}
        note={s.in_progress > 0 ? t('stats.inProgress', { n: s.in_progress }) : undefined}
      />
      <StatTile
        label={t('stats.completed')}
        value={String(s.completed)}
        note={percent !== null ? t('stats.successRate', { percent: `${percent} %` }) : undefined}
      />
      <StatTile label={t('stats.failed')} value={String(s.failed)} />
      <StatTile label={t('stats.cancelled')} value={String(s.cancelled)} />
      <StatTile
        label={t('stats.averageSpeed')}
        value={speed(s.average_speed)}
        note={t('stats.averageSpeed.hint')}
      />
      <StatTile label={t('stats.peakSpeed')} value={speed(s.peak_speed)} note={t('stats.peakSpeed.hint')} />
      <StatTile
        label={t('stats.time')}
        value={s.active_seconds > 0 ? i18n.duration(s.active_seconds) : '—'}
        note={t('stats.time.hint')}
      />
      <StatTile
        label={t('stats.largest')}
        value={s.largest ? i18n.bytes(s.largest.size) : '—'}
        note={s.largest?.name}
      />
    </div>

    {#if s.completed > 0}
      <section class={CARD} aria-labelledby="stats-size">
        <div class="flex flex-wrap items-baseline justify-between gap-x-4">
          <h2 id="stats-size" class="font-semibold tracking-tight">{t('stats.categories')}</h2>
          <p class="text-sm text-muted tabular-nums">
            {t('stats.completedSize', { size: i18n.bytes(s.completed_bytes) })}
          </p>
        </div>
        <CategoryBars categories={s.categories} />
      </section>
    {/if}

    <section class={CARD} aria-labelledby="stats-days">
      <div>
        <h2 id="stats-days" class="font-semibold tracking-tight">{t('stats.activity')}</h2>
        <p class="hint">{t('stats.activity.hint')}</p>
      </div>
      <DayChart days={s.days} />
    </section>
  {/if}
</div>
