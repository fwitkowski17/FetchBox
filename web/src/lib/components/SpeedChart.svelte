<script lang="ts">
  // Download and upload speed over the last few minutes, as two step lines. Drawn by hand (a few
  // dozen points): a chart library would be most of the size of the app.
  import { i18n, t } from '../i18n';
  import { chartPoints, niceMax, type SpeedSample } from '../util/torrents';

  let { history }: { history: readonly SpeedSample[] } = $props();

  // The window ends "now"; a clock that ticks keeps the line moving while nothing new arrives.
  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  // The plot is stretched over whatever room there is (`preserveAspectRatio="none"` with strokes that
  // do not scale); the axis labels are plain text beside it, so they are never distorted.
  const W = 1000;
  const H = 100;

  const points = $derived(chartPoints(history, now));
  const top = $derived(niceMax(Math.max(0, ...points.flatMap((p) => [p.down, p.up]))));

  function path(pick: (p: { down: number; up: number }) => number): string {
    let d = '';
    let prevY = 0;
    points.forEach((p, i) => {
      const x = p.x * W;
      const y = H - (pick(p) / top) * H;
      // A step: the speed holds until the next sample, then jumps.
      d += i === 0 ? `M${x.toFixed(1)} ${y.toFixed(1)}` : `L${x.toFixed(1)} ${prevY.toFixed(1)}L${x.toFixed(1)} ${y.toFixed(1)}`;
      prevY = y;
    });
    return d;
  }

  const downPath = $derived(path((p) => p.down));
  const upPath = $derived(path((p) => p.up));
  const ticks = $derived([1, 0.5, 0].map((f) => ({ f, label: i18n.speed(top * f) })));
  const last = $derived(points[points.length - 1]);
</script>

<figure class="grid gap-2" data-part="speed-chart">
  <figcaption class="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
    <span class="inline-flex items-center gap-1.5 font-medium text-accent">
      <span class="h-0.5 w-4 rounded bg-current"></span>{t('torrent.speed.down')}
      <span class="tabular-nums" data-part="now-down">{i18n.speed(last?.down ?? 0)}</span>
    </span>
    <span class="inline-flex items-center gap-1.5 font-medium text-success">
      <span class="h-0.5 w-4 rounded bg-current"></span>{t('torrent.speed.up')}
      <span class="tabular-nums" data-part="now-up">{i18n.speed(last?.up ?? 0)}</span>
    </span>
    <span class="ml-auto text-muted">{t('torrent.speed.window')}</span>
  </figcaption>
  {#if points.length === 0}
    <p class="grid h-44 place-items-center rounded-lg bg-surface-2 text-sm text-muted">{t('torrent.speed.empty')}</p>
  {:else}
    <div class="grid h-44 grid-cols-[auto_minmax(0,1fr)] gap-2" role="img" aria-label={t('torrent.speed.chart')}>
      <div class="relative w-16 text-right text-[11px] leading-none text-muted tabular-nums" aria-hidden="true">
        {#each ticks as tick (tick.f)}
          <span class="absolute right-0 -translate-y-1/2" style:top="{(1 - tick.f) * 100}%">{tick.label}</span>
        {/each}
      </div>
      <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" class="h-full w-full overflow-visible" aria-hidden="true">
        {#each ticks as tick (tick.f)}
          <line x1="0" x2={W} y1={(1 - tick.f) * H} y2={(1 - tick.f) * H} class="stroke-border" stroke-width="1" vector-effect="non-scaling-stroke" />
        {/each}
        <path d={upPath} fill="none" class="stroke-success" stroke-width="2" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
        <path d={downPath} fill="none" class="stroke-accent" stroke-width="2" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
      </svg>
    </div>
  {/if}
</figure>
