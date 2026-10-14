<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { setUnauthorizedHandler } from './lib/api/client';
  import ConfirmHost from './lib/components/ConfirmHost.svelte';
  import Icon, { type IconName } from './lib/components/Icon.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import { auth } from './lib/stores/auth.svelte';
  import { downloads } from './lib/stores/downloads.svelte';
  import { route, routes, type Route } from './lib/stores/route';
  import { settings } from './lib/stores/settings.svelte';
  import { theme } from './lib/stores/theme.svelte';
  import { summarize } from './lib/util/downloads';
  import { formatSpeed } from './lib/util/format';
  import AddDownload from './routes/AddDownload.svelte';
  import Downloads from './routes/Downloads.svelte';
  import Login from './routes/Login.svelte';
  import Settings from './routes/Settings.svelte';

  const NAV_ICONS: Record<Route, IconName> = { downloads: 'list', add: 'plus', settings: 'sliders' };

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
    document.title = summary.active > 0 ? `↓ ${formatSpeed(summary.speed)} · dlmanager` : 'dlmanager';
  });

  const themeIcon = $derived<IconName>(theme.pref === 'system' ? 'monitor' : theme.pref === 'light' ? 'sun' : 'moon');
  const connectionLabel = $derived(
    downloads.connection === 'live' ? 'Live' : downloads.connection === 'connecting' ? 'Reconnecting…' : 'Offline',
  );
</script>

{#if auth.state === 'loading'}
  <p class="center" role="status">Loading…</p>
{:else if auth.state === 'offline'}
  <div class="center">
    <Icon name="alert" size={32} />
    <p>Cannot reach the server.</p>
    <button class="btn primary" onclick={() => auth.check()}>Try again</button>
  </div>
{:else if auth.state === 'login'}
  <Login />
{:else}
  <header class="top">
    <span class="brand"><Icon name="down" size={20} />dlmanager</span>

    <nav class="nav" aria-label="Main">
      {#each routes as r (r.id)}
        <a
          href={'#' + r.path}
          class:active={$route === r.id}
          aria-current={$route === r.id ? 'page' : undefined}
        >
          <Icon name={NAV_ICONS[r.id]} size={22} />
          <span>{r.label}</span>
        </a>
      {/each}
    </nav>

    <div class="status">
      {#if summary.active > 0}
        <span class="speed" title="Total download speed">↓ {formatSpeed(summary.speed)}</span>
      {/if}
      <span class="conn {downloads.connection}" title={`Connection to the server: ${connectionLabel}`}>
        <span class="dot" aria-hidden="true"></span>
        <span class:sr-only={downloads.connection === 'live'}>{connectionLabel}</span>
      </span>
      <button
        class="btn icon ghost"
        onclick={() => theme.cycle()}
        aria-label={`Theme: ${theme.pref}. Activate to change.`}
        title={`Theme: ${theme.pref}`}
      >
        <Icon name={themeIcon} />
      </button>
    </div>
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

  /* Mobile: slim top bar, tab bar at the bottom. */
  .top {
    position: sticky;
    top: 0;
    z-index: 20;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    height: var(--header-h);
    padding: env(safe-area-inset-top) 0.75rem 0;
    /* Opaque on purpose: backdrop-filter would make this header the containing block of the
       `position: fixed` tab bar nested inside it and pull the bar to the top. */
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }

  .brand {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 700;
    letter-spacing: 0.01em;
  }

  .brand :global(svg) {
    color: var(--accent);
  }

  .nav {
    position: fixed;
    inset: auto 0 0 0;
    z-index: 20;
    display: flex;
    height: calc(var(--nav-h) + env(safe-area-inset-bottom));
    padding-bottom: env(safe-area-inset-bottom);
    background: var(--surface);
    border-top: 1px solid var(--border);
  }

  .nav a {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.15rem;
    color: var(--muted);
    font-size: 0.78rem;
    font-weight: 600;
    text-decoration: none;
  }

  .nav a.active {
    color: var(--accent);
  }

  .status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .speed {
    color: var(--accent);
    font-weight: 600;
    font-size: 0.9rem;
    font-variant-numeric: tabular-nums;
  }

  .conn {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--muted);
    font-size: 0.8rem;
  }

  .dot {
    width: 0.6rem;
    height: 0.6rem;
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

  main {
    max-width: 48rem;
    margin: 0 auto;
    padding: 1rem 1rem calc(var(--nav-h) + 2rem + env(safe-area-inset-bottom));
  }

  /* Desktop: navigation moves into the top bar. */
  @media (min-width: 48rem) {
    .top {
      padding: 0 1.25rem;
    }

    .nav {
      position: static;
      height: auto;
      padding: 0;
      background: transparent;
      border: 0;
      gap: 0.25rem;
    }

    .nav a {
      flex: none;
      flex-direction: row;
      gap: 0.45rem;
      padding: 0.45rem 0.9rem;
      border-radius: var(--radius-sm);
      font-size: 0.95rem;
    }

    .nav a:hover {
      background: var(--surface-2);
    }

    .nav a.active {
      background: var(--accent-soft);
    }

    main {
      padding-bottom: 3rem;
    }
  }
</style>
