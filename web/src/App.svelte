<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { setUnauthorizedHandler } from './lib/api/client';
  import ConfirmHost from './lib/components/ConfirmHost.svelte';
  import Icon from './lib/components/Icon.svelte';
  import Sidebar from './lib/components/Sidebar.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import { i18n, t } from './lib/i18n';
  import { auth } from './lib/stores/auth.svelte';
  import { downloads } from './lib/stores/downloads.svelte';
  import { media } from './lib/stores/media.svelte';
  import { route } from './lib/stores/route';
  import { settings } from './lib/stores/settings.svelte';
  import { theme } from './lib/stores/theme.svelte';
  import { summarize } from './lib/util/downloads';
  import AddDownload from './routes/AddDownload.svelte';
  import Downloads from './routes/Downloads.svelte';
  import Login from './routes/Login.svelte';
  import Settings from './routes/Settings.svelte';

  onMount(() => {
    theme.init();
    setUnauthorizedHandler(() => auth.expire());
    void auth.check();
  });

  // Data is only fetched (and the event stream only open) while signed in.
  $effect(() => {
    if (auth.state === 'ready') {
      untrack(() => {
        downloads.start({
          onClosed: () => void auth.check(),
          onSettings: (s) => settings.received(s),
        });
        if (!settings.value) void settings.load();
      });
    } else {
      untrack(() => {
        downloads.stop();
        settings.clear();
      });
    }
  });

  const summary = $derived(summarize(downloads.items));

  // Current speed in the tab title, handy when the tab is in the background.
  $effect(() => {
    document.title = summary.active > 0 ? `↓ ${i18n.speed(summary.speed)} · dlmanager` : 'dlmanager';
  });

  // ---- the menu: a permanent panel on wide screens, a drawer on narrow ones
  let menuOpen = $state(false);
  let menuButton: HTMLButtonElement | undefined = $state();
  let sidebar: HTMLElement | undefined = $state();

  async function openMenu(): Promise<void> {
    menuOpen = true;
    await tick();
    sidebar?.querySelector<HTMLElement>('a, button')?.focus();
  }

  async function closeMenu(): Promise<void> {
    if (!menuOpen) return;
    menuOpen = false;
    // The page behind the drawer is `inert` until the DOM updates; focus cannot enter it before.
    await tick();
    menuButton?.focus();
  }

  // Going to another page, or widening the window, leaves nothing to close.
  $effect(() => {
    void $route;
    if (media.desktop) untrack(() => (menuOpen = false));
  });

  const drawerOpen = $derived(menuOpen && !media.desktop);
  const connectionLabel = $derived(t(`connection.${downloads.connection}`));
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && drawerOpen && closeMenu()} />

{#if auth.state === 'loading'}
  <p class="center" role="status">{t('common.loading')}</p>
{:else if auth.state === 'offline'}
  <div class="center">
    <Icon name="alert" size={32} />
    <p>{t('app.offline')}</p>
    <button class="btn primary" onclick={() => auth.check()}>{t('common.tryAgain')}</button>
  </div>
{:else if auth.state === 'login'}
  <Login />
{:else}
  <div class="shell">
    <Sidebar
      bind:el={sidebar}
      open={drawerOpen}
      inert={!media.desktop && !menuOpen}
      onclose={closeMenu}
    />
    {#if drawerOpen}
      <button class="scrim" tabindex="-1" aria-label={t('nav.close')} onclick={closeMenu}></button>
    {/if}

    <div class="content" inert={drawerOpen}>
      <header class="topbar">
        <button
          bind:this={menuButton}
          class="btn icon ghost"
          aria-label={t('nav.open')}
          aria-controls="sidebar"
          aria-expanded={drawerOpen}
          onclick={openMenu}
        >
          <Icon name="menu" />
        </button>
        <span class="brand"><Icon name="down" size={20} />dlmanager</span>
        <span class="grow"></span>
        {#if summary.active > 0}
          <span class="speed" title={t('app.totalSpeed')}>↓ {i18n.speed(summary.speed)}</span>
        {/if}
        <span class="dot {downloads.connection}" role="img" aria-label={connectionLabel} title={connectionLabel}></span>
      </header>

      <main>
        {#if $route === 'downloads'}
          <Downloads />
        {:else if $route === 'add'}
          <AddDownload />
        {:else}
          <Settings />
        {/if}
      </main>
    </div>

    {#if !media.desktop && $route === 'downloads'}
      <a class="fab" href="#/add" aria-label={t('nav.add')} title={t('nav.add')}>
        <Icon name="plus" size={26} />
      </a>
    {/if}
  </div>
{/if}

<ConfirmHost />
<Toasts />

<style>
  .center {
    display: grid;
    justify-items: center;
    align-content: center;
    gap: 0.75rem;
    min-height: 100dvh;
    color: var(--muted);
    text-align: center;
  }

  .shell {
    min-height: 100dvh;
  }

  .content {
    min-width: 0;
  }

  main {
    max-width: 76rem;
    margin: 0 auto;
    padding: 1rem 1rem calc(5.5rem + env(safe-area-inset-bottom));
  }

  /* ---- narrow screens: slim top bar, drawer, floating add button */
  .topbar {
    position: sticky;
    top: 0;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    height: var(--header-h);
    padding: env(safe-area-inset-top) 0.5rem 0;
    /* Opaque on purpose: a `backdrop-filter` would make the bar the containing block of
       fixed-position descendants and break them. */
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }

  .brand {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 700;
  }

  .brand :global(svg) {
    color: var(--accent);
  }

  .grow {
    flex: 1;
  }

  .speed {
    color: var(--accent);
    font-weight: 600;
    font-size: 0.9rem;
    font-variant-numeric: tabular-nums;
  }

  .dot {
    width: 0.65rem;
    height: 0.65rem;
    margin-right: 0.5rem;
    border-radius: 50%;
    background: var(--success);
  }

  .dot.connecting {
    background: var(--warning);
    animation: pulse 1.2s ease-in-out infinite;
  }

  .dot.offline {
    background: var(--danger);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .scrim {
    position: fixed;
    inset: 0;
    z-index: 30;
    border: 0;
    background: var(--backdrop);
    cursor: default;
  }

  .fab {
    position: fixed;
    right: calc(1rem + env(safe-area-inset-right));
    bottom: calc(1rem + env(safe-area-inset-bottom));
    z-index: 10;
    display: grid;
    place-items: center;
    width: 3.5rem;
    height: 3.5rem;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-contrast);
    box-shadow: 0 4px 16px rgb(0 0 0 / 35%);
  }

  /* ---- wide screens: permanent side panel, no top bar */
  @media (min-width: 64rem) {
    .shell {
      display: grid;
      grid-template-columns: var(--sidebar-w) minmax(0, 1fr);
    }

    .topbar {
      display: none;
    }

    main {
      padding: 1.5rem 2rem 3rem;
    }
  }
</style>
