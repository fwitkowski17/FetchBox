<script lang="ts">
  import { CATEGORIES, type Category } from '../api/types';
  import { i18n, t } from '../i18n';
  import { downloads } from '../stores/downloads.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { route, routes, type Route } from '../stores/route';
  import { theme, type ThemePref } from '../stores/theme.svelte';
  import { GROUPS, countCategories, countGroups, summarize, type Group } from '../util/downloads';
  import { GROUP_COLOR, GROUP_ICON } from '../util/groupStyle';
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
  const DOT: Record<string, string> = {
    live: 'bg-success',
    connecting: 'animate-pulse bg-warning',
    offline: 'bg-danger',
  };

  const NAV_ICONS: Record<Route, IconName> = { downloads: 'list', add: 'plus', settings: 'sliders' };
  const THEME_ICONS: Record<ThemePref, IconName> = {
    system: 'monitor',
    light: 'sun',
    dark: 'moon',
    black: 'contrast',
  };

  const filter = $derived({ group: prefs.group, category: prefs.category, query: prefs.query });
  const groupCounts = $derived(countGroups(downloads.items, filter));
  const categoryCounts = $derived(countCategories(downloads.items, filter));
  const summary = $derived(summarize(downloads.items));
  const connectionLabel = $derived(t(`connection.${downloads.connection}`));

  /** Filters apply to the list, so choosing one from another page goes back to the list. */
  function showList(): void {
    if ($route !== 'downloads') location.hash = '#/';
    onclose();
  }

  function pickGroup(group: Group): void {
    prefs.setGroup(group);
    showList();
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
      dlmanager
    </span>
    <button class="btn icon ghost lg:hidden" aria-label={t('nav.close')} onclick={onclose}>
      <Icon name="x" />
    </button>
  </div>

  <nav aria-label={t('nav.main')}>
    <ul class="grid gap-0.5">
      {#each routes as r (r.id)}
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

  <section aria-labelledby="filters-status">
    <h2 id="filters-status" class={HEADING}>{t('filters.status')}</h2>
    <ul class="grid gap-0.5">
      {#each GROUPS as g (g.id)}
        <li>
          <button class={ITEM} aria-pressed={prefs.group === g.id} onclick={() => pickGroup(g.id)}>
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
        <button class={ITEM} aria-pressed={prefs.category === 'all'} onclick={() => pickCategory('all')}>
          {t('downloads.filter.allCategories')}<span class={COUNT}>{categoryCounts.all}</span>
        </button>
      </li>
      {#each CATEGORIES as c (c)}
        <li>
          <button class={ITEM} aria-pressed={prefs.category === c} onclick={() => pickCategory(c)}>
            <CategoryIcon category={c} size={18} />{t(`category.${c}`)}<span class={COUNT}>{categoryCounts[c]}</span>
          </button>
        </li>
      {/each}
    </ul>
  </section>

  <!-- Stays in view while the lists above scroll (short windows, phones). -->
  <div class="sticky -bottom-4 mt-auto -mb-4 -mx-3 grid gap-1 border-t border-border bg-surface px-3 pt-3 pb-4">
    {#if summary.active > 0}
      <p class="px-3 text-sm font-semibold text-accent tabular-nums" title={t('app.totalSpeed')}>
        ↓ {i18n.speed(summary.speed)}
      </p>
    {/if}
    <p
      class="flex items-center gap-2 px-3 text-sm text-muted"
      title={t('connection.title', { state: connectionLabel })}
    >
      <span class={['size-2 rounded-full', DOT[downloads.connection]]} aria-hidden="true"></span>{connectionLabel}
    </p>
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
