<script lang="ts">
  // The last days as columns: data downloaded per day. One series, so no legend (the title says
  // what is plotted); a hairline grid, thin columns with a rounded top, the values on the axis and
  // in a tooltip, never a number on every column. A table below (for screen readers) repeats it.
  import type { DayStat } from '../api/types';
  import { i18n, t } from '../i18n';
  import { barShare, niceAxis, trimAxisZero } from '../util/stats';

  let { days }: { days: readonly DayStat[] } = $props();

  const axis = $derived(niceAxis(Math.max(0, ...days.map((d) => d.bytes))));
  const today = $derived(days.length - 1);

  function date(start: number, options: Intl.DateTimeFormatOptions): string {
    return new Intl.DateTimeFormat(i18n.locale, options).format(new Date(start * 1000));
  }
</script>

<div class="grid grid-cols-[auto_1fr] gap-x-2" data-part="day-chart">
  <!-- The axis: the top and the middle; zero is the baseline. -->
  <div class="relative h-40 w-14 text-right text-xs text-muted tabular-nums" aria-hidden="true">
    {#each [axis.ticks[2], axis.ticks[1]] as tick, i (tick)}
      <span class="absolute right-0 -translate-y-1/2" style:top="{i * 50}%">{trimAxisZero(i18n.bytes(tick))}</span>
    {/each}
    <span class="absolute right-0 bottom-0 translate-y-1/2">0</span>
  </div>

  <div class="relative">
    <!-- Hairline grid: top, middle, baseline. -->
    <div class="pointer-events-none absolute inset-x-0 top-0 h-40" aria-hidden="true">
      <span class="absolute inset-x-0 top-0 border-t border-border"></span>
      <span class="absolute inset-x-0 top-1/2 border-t border-border"></span>
      <span class="absolute inset-x-0 bottom-0 border-t border-border"></span>
    </div>

    <ul class="relative flex h-40 items-stretch" aria-label={t('stats.activity')}>
      {#each days as d, i (d.start)}
        {@const share = barShare(d.bytes, axis.top)}
        <li class="group relative flex flex-1 items-end justify-center">
          <!-- The whole column is the target, much bigger than a thin bar. -->
          <button
            type="button"
            class="peer absolute inset-0 cursor-default rounded outline-offset-[-2px] focus-visible:outline-2 focus-visible:outline-accent"
            aria-label={t('stats.day.label', {
              date: date(d.start, { weekday: 'long', day: 'numeric', month: 'long' }),
              size: i18n.bytes(d.bytes),
              downloads: t('stats.downloadsCount', { n: d.count }),
            })}
          ></button>
          {#if share > 0}
            <span
              class="pointer-events-none w-full max-w-6 rounded-t-[4px] bg-accent transition-[height,opacity] duration-200 group-hover:opacity-80 peer-focus-visible:opacity-80"
              style:height="{share * 100}%"
            ></span>
          {/if}
          <!-- Just above the top of its bar (or of the baseline, for an empty day). -->
          <span
            class={[
              'pointer-events-none absolute z-10 hidden w-max max-w-56 rounded-lg border border-border bg-surface px-3 py-2 text-left text-sm shadow-card group-hover:block peer-focus-visible:block',
              i < days.length / 2 ? 'left-0' : 'right-0',
            ]}
            style:bottom="calc({share * 100}% + 0.5rem)"
            role="presentation"
          >
            <span class="block font-semibold">{date(d.start, { weekday: 'short', day: 'numeric', month: 'short' })}</span>
            <span class="block tabular-nums">{i18n.bytes(d.bytes)}</span>
            <span class="block text-muted">{t('stats.downloadsCount', { n: d.count })}</span>
          </span>
        </li>
      {/each}
    </ul>

    <!-- Day names under the columns; the narrow letter on a phone, today in bold. -->
    <ul class="mt-1.5 flex text-xs text-muted" aria-hidden="true">
      {#each days as d, i (d.start)}
        <li class={['flex-1 text-center', i === today && 'font-semibold text-text']}>
          <span class="sm:hidden">{date(d.start, { weekday: 'narrow' })}</span>
          <span class="hidden sm:inline">{date(d.start, { weekday: 'short' })}</span>
          <span class="block tabular-nums">{date(d.start, { day: 'numeric' })}</span>
        </li>
      {/each}
    </ul>
  </div>
</div>

<!-- In a box of its own: a table ignores `sr-only` and would stretch the page. -->
<div class="sr-only">
<table>
  <caption>{t('stats.activity')}</caption>
  <thead>
    <tr>
      <th scope="col">{t('stats.table.day')}</th>
      <th scope="col">{t('stats.table.data')}</th>
      <th scope="col">{t('stats.table.downloads')}</th>
    </tr>
  </thead>
  <tbody>
    {#each days as d (d.start)}
      <tr>
        <th scope="row">{date(d.start, { dateStyle: 'full' })}</th>
        <td>{i18n.bytes(d.bytes)}</td>
        <td>{d.count}</td>
      </tr>
    {/each}
  </tbody>
</table>
</div>
