<script lang="ts">
  import { CATEGORIES, type Category } from '../api/types';
  import { i18n, t } from '../i18n';
  import { downloads } from '../stores/downloads.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { torrents } from '../stores/torrents.svelte';
  import { TORRENT_GROUPS, countTorrentGroups, summarizeTorrents, type TorrentGroup } from '../util/torrents';
  import { route, routes, type Route } from '../stores/route';
  import { theme, type ThemePref } from '../stores/theme.svelte';
  import { GROUPS, countCategories, countGroups, summarize, type Group } from '../util/downloads';
  import { GROUP_COLOR, GROUP_ICON, TORRENT_GROUP_COLOR, TORRENT_GROUP_ICON } from '../util/groupStyle';
  import CategoryIcon from './CategoryIcon.svelte';
  import DiskRing from './DiskRing.svelte';
  import Icon, { type IconName } from './Icon.svelte';

  interface Props {
    /** Drawer state; only matters below the desktop breakpoint. */
    open: boolean;
    /** Takes the panel out of reach (focus and screen readers) while it is a closed drawer. */
    inert: boolean;
    onclose: () => void;
    el?: HTMLElement | undefined;
  }

  let { open, inert, onclose, el = $bindable() }: Props = $props();

  // One look for navigation links and filter buttons. `aria-pressed` / `aria-current` mark the
  // selected one, so no extra class juggling is needed.
  const ITEM =
    'flex min-h-9 w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left font-medium text-text no-underline transition-colors hover:bg-surface-2 aria-pressed:bg-accent-soft aria-pressed:font-semibold aria-pressed:text-accent aria-[current=page]:bg-accent-soft aria-[current=page]:font-semibold aria-[current=page]:text-accent';
  const COUNT = 'ml-auto text-sm font-normal tabular-nums opacity-60';
  const HEADING = 'mb-1 px-3 text-[0.7rem] font-bold tracking-widest text-muted uppercase';
  // Downloads and torrents are the two sections, chosen at the top; the lists are reached through
  // their status filters ("All"). Only the other pages are links down here, above the disk ring.
  const pageRoutes = routes.filter((r) => r.id !== 'downloads' && r.id !== 'torrents');
  const SECTIONS = routes.filter((r) => r.id === 'downloads' || r.id === 'torrents');
  const NAV_ICONS: Record<Route, IconName> = {
    downloads: 'list',
    torrents: 'magnet',
    stats: 'chart',
    settings: 'sliders',
  };
  const THEME_ICONS: Record<ThemePref, IconName> = {
    system: 'monitor',
    light: 'sun',
    dark: 'moon',
    black: 'contrast',
  };

  /** The filters belong to a list: on the other pages none of them is marked as current. */
  const onList = $derived($route === 'downloads');
  const onTorrents = $derived($route === 'torrents');
  // Statistics and Settings belong to neither list: the panel keeps showing the filters of the
  // section that was open last, so the way back is one click.
  let lastSection = $state<'downloads' | 'torrents'>($route === 'torrents' ? 'torrents' : 'downloads');
  $effect(() => {
    if ($route === 'downloads' || $route === 'torrents') lastSection = $route;
  });
  const section = $derived(lastSection);
  const filter = $derived({ group: prefs.group, category: prefs.category, query: prefs.query });
  const groupCounts = $derived(countGroups(downloads.items, filter));
  const categoryCounts = $derived(countCategories(downloads.items, filter));
  const summary = $derived(summarize(downloads.items));
  const torrentSummary = $derived(summarizeTorrents(torrents.items));
  const torrentCounts = $derived(countTorrentGroups(torrents.items, torrents.query));

  /** Filters apply to the list, so choosing one from another page goes back to the list. */
  function showList(): void {
    if ($route !== 'downloads') location.hash = '#/';
    onclose();
  }

  function pickGroup(group: Group): void {
    prefs.setGroup(group);
    showList();
  }

  function pickTorrentGroup(group: TorrentGroup): void {
    torrents.setGroup(group);
    if ($route !== 'torrents') location.hash = '#/torrents';
    onclose();
  }

  function pickCategory(category: Category | 'all'): void {
    prefs.setCategory(category);
    showList();
  }
</script>

<aside
  id="sidebar"
  bind:this={el}
  {inert}
  class={[
    'flex flex-col gap-4 overflow-y-auto border-r border-border bg-surface px-3 py-4',
    // Wide screens: a permanent panel next to the content.
    'lg:sticky lg:top-0 lg:h-dvh',
    // Narrow screens: a drawer. `visibility` flips after the slide-out so it is not cut short.
    'max-lg:fixed max-lg:inset-y-0 max-lg:left-0 max-lg:z-40 max-lg:w-[min(20rem,86vw)] max-lg:pt-[calc(1rem+env(safe-area-inset-top))] max-lg:pb-[calc(1rem+env(safe-area-inset-bottom))]',
    open
      ? 'max-lg:visible max-lg:translate-x-0 max-lg:shadow-2xl max-lg:[transition:transform_0.2s_ease,visibility_0s]'
      : 'max-lg:invisible max-lg:-translate-x-[105%] max-lg:[transition:transform_0.2s_ease,visibility_0s_linear_0.2s]',
  ]}
>
  <div class="flex items-center justify-between pr-1 pl-2">
    <span class="flex items-center gap-2.5 text-lg font-bold tracking-tight">
      <span class="grid size-8 place-items-center rounded-lg bg-accent text-accent-contrast">
        <Icon name="down" size={18} />
      </span>
      FetchBox
    </span>
    <button class="btn icon ghost lg:hidden" aria-label={t('nav.close')} onclick={onclose}>
      <Icon name="x" />
    </button>
  </div>

  <!-- The two sections come first: what the panel filters below depends on the choice. -->
  <nav aria-label={t('nav.sections')}>
    <ul class="grid grid-cols-2 gap-1 rounded-xl bg-surface-2 p-1" data-part="sections">
      {#each SECTIONS as r (r.id)}
        <li>
          <a
            class="flex min-h-9 items-center justify-center gap-2 rounded-lg px-2 py-1.5 text-sm font-semibold text-muted no-underline transition-colors hover:text-text aria-[current=page]:bg-surface aria-[current=page]:text-accent aria-[current=page]:shadow-card"
            href={'#' + r.path}
            aria-current={$route === r.id ? 'page' : undefined}
            data-section={r.id}
            data-last={section === r.id ? '' : undefined}
            onclick={onclose}
          >
            <Icon name={NAV_ICONS[r.id]} size={18} />{t(`nav.${r.id}`)}
            {#if r.id === 'torrents' && torrentSummary.active > 0}
              <span class="text-xs font-normal tabular-nums opacity-70" data-part="torrent-count">{torrentSummary.active}</span>
            {/if}
          </a>
        </li>
      {/each}
    </ul>
  </nav>

  {#if section === 'torrents'}
    <section aria-labelledby="filters-status" data-part="torrent-filters">
      <h2 id="filters-status" class={HEADING}>{t('filters.status')}</h2>
      <ul class="grid gap-0.5">
        {#each TORRENT_GROUPS as g (g)}
          <li>
            <button class={ITEM} aria-pressed={onTorrents && torrents.group === g} onclick={() => pickTorrentGroup(g)}>
              <span class={TORRENT_GROUP_COLOR[g]}><Icon name={TORRENT_GROUP_ICON[g]} size={18} /></span>
              {t(`torrentGroup.${g}`)}<span class={COUNT}>{torrentCounts[g]}</span>
            </button>
          </li>
        {/each}
      </ul>
    </section>
  {:else}
  <section aria-labelledby="filters-status">
    <h2 id="filters-status" class={HEADING}>{t('filters.status')}</h2>
    <ul class="grid gap-0.5">
      {#each GROUPS as g (g.id)}
        <li>
          <button class={ITEM} aria-pressed={onList && prefs.group === g.id} onclick={() => pickGroup(g.id)}>
            <span class={GROUP_COLOR[g.id]}><Icon name={GROUP_ICON[g.id]} size={18} /></span>
            {t(`group.${g.id}`)}<span class={COUNT}>{groupCounts[g.id]}</span>
          </button>
        </li>
      {/each}
    </ul>
  </section>

  <section aria-labelledby="filters-categories">
    <h2 id="filters-categories" class={HEADING}>{t('filters.categories')}</h2>
    <ul class="grid gap-0.5">
      <li>
        <button class={ITEM} aria-pressed={onList && prefs.category === 'all'} onclick={() => pickCategory('all')}>
          {t('downloads.filter.allCategories')}<span class={COUNT}>{categoryCounts.all}</span>
        </button>
      </li>
      {#each CATEGORIES as c (c)}
        <li>
          <button class={ITEM} aria-pressed={onList && prefs.category === c} onclick={() => pickCategory(c)}>
            <CategoryIcon category={c} size={18} />{t(`category.${c}`)}<span class={COUNT}>{categoryCounts[c]}</span>
          </button>
        </li>
      {/each}
    </ul>
  </section>
  {/if}

  <!-- Stays in view while the lists above scroll (short windows, phones). -->
  <div class="sticky -bottom-4 mt-auto -mb-4 -mx-3 grid gap-1 border-t border-border bg-surface px-3 pt-3 pb-4">
    {#if section === 'torrents' && torrentSummary.active > 0}
      <p class="flex gap-3 px-3 text-sm font-semibold tabular-nums" title={t('app.totalSpeed')}>
        <span class="text-accent">↓ {i18n.speed(torrentSummary.down)}</span>
        <span class="text-success">↑ {i18n.speed(torrentSummary.up)}</span>
      </p>
    {:else if section === 'downloads' && summary.active > 0}
      <p class="px-3 text-sm font-semibold text-accent tabular-nums" title={t('app.totalSpeed')}>
        ↓ {i18n.speed(summary.speed)}
      </p>
    {/if}
    <nav aria-label={t('nav.main')}>
      <ul class="grid gap-0.5">
        {#each pageRoutes as r (r.id)}
          <li>
            <a
              class={ITEM}
              href={'#' + r.path}
              aria-current={$route === r.id ? 'page' : undefined}
              onclick={onclose}
            >
              <Icon name={NAV_ICONS[r.id]} size={20} />{t(`nav.${r.id}`)}
            </a>
          </li>
        {/each}
      </ul>
    </nav>
    <button
      class={ITEM}
      onclick={() => theme.cycle()}
      aria-label={t('theme.button', { theme: t(`theme.${theme.pref}`) })}
    >
      <Icon name={THEME_ICONS[theme.pref]} size={20} />{t(`theme.${theme.pref}`)}
    </button>
    <DiskRing />
  </div>
</aside>
