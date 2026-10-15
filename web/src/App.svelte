<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { fade, fly, scale } from 'svelte/transition';
  import { setUnauthorizedHandler } from './lib/api/client';
  import AddDownloadDialog from './lib/components/AddDownloadDialog.svelte';
  import ConfirmHost from './lib/components/ConfirmHost.svelte';
  import DeleteDownloadDialog from './lib/components/DeleteDownloadDialog.svelte';
  import Icon from './lib/components/Icon.svelte';
  import ResumeSpaceDialog from './lib/components/ResumeSpaceDialog.svelte';
  import Sidebar from './lib/components/Sidebar.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import { i18n, t } from './lib/i18n';
  import { addDialog } from './lib/stores/addDialog.svelte';
  import { auth } from './lib/stores/auth.svelte';
  import { disk } from './lib/stores/disk.svelte';
  import { downloads } from './lib/stores/downloads.svelte';
  import { media } from './lib/stores/media.svelte';
  import { route } from './lib/stores/route';
  import { settings } from './lib/stores/settings.svelte';
  import { theme } from './lib/stores/theme.svelte';
  import { summarize } from './lib/util/downloads';
  import { motion } from './lib/util/motion';
  import Downloads from './routes/Downloads.svelte';
  import Login from './routes/Login.svelte';
  import Settings from './routes/Settings.svelte';
  import Setup from './routes/Setup.svelte';

  /** Old links and bookmarks to `#/add` now open the dialog over the list. */
  function openLegacyAddLink(): void {
    if (location.hash !== '#/add') return;
    history.replaceState(null, '', '#/');
    addDialog.show();
  }

  onMount(() => {
    openLegacyAddLink();
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
        disk.start();
      });
    } else {
      untrack(() => {
        downloads.stop();
        settings.clear();
        disk.stop();
      });
    }
  });

  const summary = $derived(summarize(downloads.items));

  // The disk figures depend on which downloads exist and which are still unfinished: refresh when a
  // download is added, finishes, fails or is removed (not on every progress tick).
  const diskKey = $derived(
    `${downloads.items.length}/${downloads.items.filter((d) => d.status !== 'downloading' && d.status !== 'queued').length}`,
  );
  $effect(() => {
    void diskKey;
    if (auth.state === 'ready') untrack(() => void disk.refresh());
  });

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

  const DOT: Record<string, string> = {
    live: 'bg-success',
    connecting: 'animate-pulse bg-warning',
    offline: 'bg-danger',
  };

  const drawerOpen = $derived(menuOpen && !media.desktop);
  const connectionLabel = $derived(t(`connection.${downloads.connection}`));
</script>

<svelte:window onhashchange={openLegacyAddLink} onkeydown={(e) => e.key === 'Escape' && drawerOpen && closeMenu()} />

{#if auth.state === 'loading'}
  <p class="grid min-h-dvh place-content-center text-muted" role="status">{t('common.loading')}</p>
{:else if auth.state === 'offline'}
  <div class="grid min-h-dvh content-center justify-items-center gap-3 text-center text-muted">
    <Icon name="alert" size={32} />
    <p>{t('app.offline')}</p>
    <button class="btn primary" onclick={() => auth.check()}>{t('common.tryAgain')}</button>
  </div>
{:else if auth.wizard}
  <!-- Also while signed in: the wizard continues after the account step. -->
  <Setup />
{:else if auth.state === 'login'}
  <Login />
{:else}
  <div class="min-h-dvh lg:grid lg:grid-cols-[var(--sidebar-w)_minmax(0,1fr)]">
    <Sidebar
      bind:el={sidebar}
      open={drawerOpen}
      inert={!media.desktop && !menuOpen}
      onclose={closeMenu}
    />
    {#if drawerOpen}
      <button
        transition:fade={{ duration: motion(160) }}
        class="fixed inset-0 z-30 cursor-default bg-backdrop"
        tabindex="-1"
        aria-label={t('nav.close')}
        data-part="scrim"
        onclick={closeMenu}
      ></button>
    {/if}

    <div class="min-w-0" inert={drawerOpen} data-part="content">
      <!-- Opaque on purpose: a `backdrop-filter` would make the bar the containing block of
           fixed-position descendants and break them. -->
      <header
        class="sticky top-0 z-20 flex h-(--header-h) items-center gap-2 border-b border-border bg-bg px-2 pt-[env(safe-area-inset-top)] lg:hidden"
        data-part="topbar"
      >
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
        <span class="flex items-center gap-2 font-bold tracking-tight">
          <span class="grid size-7 place-items-center rounded-md bg-accent text-accent-contrast">
            <Icon name="down" size={16} />
          </span>
          dlmanager
        </span>
        <span class="flex-1"></span>
        {#if summary.active > 0}
          <span class="text-sm font-semibold text-accent tabular-nums" title={t('app.totalSpeed')}>
            ↓ {i18n.speed(summary.speed)}
          </span>
        {/if}
        <!-- Normally just a dot; the words appear when something is wrong. -->
        <span class="mr-2 flex items-center gap-1.5 text-sm text-muted" title={connectionLabel}>
          <span class={['size-2.5 rounded-full', DOT[downloads.connection]]} aria-hidden="true"></span>
          <span class={[downloads.connection === 'live' && 'sr-only']}>{connectionLabel}</span>
        </span>
      </header>

      <main class="mx-auto max-w-[76rem] px-4 pt-4 pb-[calc(5.5rem+env(safe-area-inset-bottom))] lg:px-8 lg:pt-6 lg:pb-12">
        {#key $route}
          <div in:fly={{ y: 8, duration: motion(180) }}>
            {#if $route === 'downloads'}
              <Downloads />
            {:else}
              <Settings />
            {/if}
          </div>
        {/key}
      </main>
    </div>

    {#if !media.desktop && $route === 'downloads'}
      <button
        in:scale={{ start: 0.6, duration: motion(200) }}
        out:fade={{ duration: motion(120) }}
        class="fixed right-[calc(1rem+env(safe-area-inset-right))] bottom-[calc(1rem+env(safe-area-inset-bottom))] z-10 grid size-14 place-items-center rounded-full bg-accent text-accent-contrast shadow-lg transition-transform active:scale-90"
        aria-label={t('nav.add')}
        title={t('nav.add')}
        data-part="fab"
        onclick={() => addDialog.show()}
      >
        <Icon name="plus" size={26} />
      </button>
    {/if}
  </div>

  <AddDownloadDialog />
  <DeleteDownloadDialog />
{/if}

<ConfirmHost />
<ResumeSpaceDialog />
<Toasts />
