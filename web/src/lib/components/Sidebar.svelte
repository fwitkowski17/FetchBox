<script lang="ts">
  import { CATEGORIES, type Category } from '../api/types';
  import { i18n, t } from '../i18n';
  import { downloads } from '../stores/downloads.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { route, routes, type Route } from '../stores/route';
  import { theme, type ThemePref } from '../stores/theme.svelte';
  import { GROUPS, countCategories, countGroups, summarize, type Group } from '../util/downloads';
  import CategoryIcon from './CategoryIcon.svelte';
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

<aside id="sidebar" class="sidebar" class:open {inert} bind:this={el}>
  <div class="head">
    <span class="brand"><Icon name="down" size={20} />dlmanager</span>
    <button class="btn icon ghost close" aria-label={t('nav.close')} onclick={onclose}>
      <Icon name="x" />
    </button>
  </div>

  <nav aria-label={t('nav.main')}>
    <ul>
      {#each routes as r (r.id)}
        <li>
          <a
            class="item"
            class:active={$route === r.id}
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
    <h2 id="filters-status">{t('filters.status')}</h2>
    <ul>
      {#each GROUPS as g (g.id)}
        <li>
          <button class="item" aria-pressed={prefs.group === g.id} onclick={() => pickGroup(g.id)}>
            {t(`group.${g.id}`)}<span class="count">{groupCounts[g.id]}</span>
          </button>
        </li>
      {/each}
    </ul>
  </section>

  <section aria-labelledby="filters-categories">
    <h2 id="filters-categories">{t('filters.categories')}</h2>
    <ul>
      <li>
        <button class="item" aria-pressed={prefs.category === 'all'} onclick={() => pickCategory('all')}>
          {t('downloads.filter.allCategories')}<span class="count">{categoryCounts.all}</span>
        </button>
      </li>
      {#each CATEGORIES as c (c)}
        <li>
          <button class="item" aria-pressed={prefs.category === c} onclick={() => pickCategory(c)}>
            <CategoryIcon category={c} size={18} />{t(`category.${c}`)}<span class="count">{categoryCounts[c]}</span>
          </button>
        </li>
      {/each}
    </ul>
  </section>

  <div class="foot">
    {#if summary.active > 0}
      <p class="speed" title={t('app.totalSpeed')}>↓ {i18n.speed(summary.speed)}</p>
    {/if}
    <p class="conn {downloads.connection}" title={t('connection.title', { state: connectionLabel })}>
      <span class="dot" aria-hidden="true"></span>{connectionLabel}
    </p>
    <button
      class="item"
      onclick={() => theme.cycle()}
      aria-label={t('theme.button', { theme: t(`theme.${theme.pref}`) })}
    >
      <Icon name={THEME_ICONS[theme.pref]} size={20} />{t(`theme.${theme.pref}`)}
    </button>
  </div>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 1.1rem;
    padding: 1rem 0.75rem;
    overflow-y: auto;
    background: var(--surface);
    border-right: 1px solid var(--border);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0.25rem 0 0.7rem;
  }

  .brand {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 1.1rem;
    font-weight: 700;
    letter-spacing: 0.01em;
  }

  .brand :global(svg) {
    color: var(--accent);
  }

  ul {
    display: grid;
    gap: 0.15rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  h2 {
    margin: 0 0 0.35rem 0.7rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    width: 100%;
    min-height: 2.35rem;
    padding: 0.3rem 0.7rem;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    font-weight: 500;
    text-align: left;
    text-decoration: none;
    cursor: pointer;
  }

  .item:hover {
    background: var(--surface-2);
  }

  .item.active,
  .item[aria-pressed='true'] {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }

  .count {
    margin-left: auto;
    color: var(--muted);
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
  }

  .item[aria-pressed='true'] .count {
    color: inherit;
  }

  /* Stays in view while the lists above scroll (short windows, phones). */
  .foot {
    position: sticky;
    bottom: -1rem;
    display: grid;
    gap: 0.35rem;
    margin: auto -0.75rem -1rem;
    padding: 0.75rem 0.75rem 1rem;
    background: var(--surface);
    border-top: 1px solid var(--border);
  }

  .speed,
  .conn {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0;
    padding: 0 0.7rem;
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
  }

  .speed {
    color: var(--accent);
    font-weight: 600;
  }

  .conn {
    color: var(--muted);
  }

  .dot {
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
    background: var(--success);
  }

  .conn.connecting .dot {
    background: var(--warning);
    animation: pulse 1.2s ease-in-out infinite;
  }

  .conn.offline .dot {
    background: var(--danger);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  /* Wide screens: a permanent panel next to the content. */
  @media (min-width: 64rem) {
    .sidebar {
      position: sticky;
      top: 0;
      height: 100dvh;
    }

    .close {
      display: none;
    }
  }

  /* Narrow screens: slides in over the content. */
  @media (max-width: 63.999rem) {
    .sidebar {
      position: fixed;
      inset: 0 auto 0 0;
      z-index: 40;
      width: min(20rem, 86vw);
      padding-top: calc(1rem + env(safe-area-inset-top));
      padding-bottom: calc(1rem + env(safe-area-inset-bottom));
      transform: translateX(-105%);
      /* `visibility` flips after the slide-out, so the animation is not cut short. */
      transition:
        transform 0.2s ease,
        visibility 0s linear 0.2s;
      visibility: hidden;
    }

    .sidebar.open {
      transform: none;
      visibility: visible;
      transition:
        transform 0.2s ease,
        visibility 0s;
      box-shadow: 0 0 48px rgb(0 0 0 / 45%);
    }
  }
</style>
