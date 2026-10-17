<script lang="ts">
  // The panel at the bottom of the page, as in a BitTorrent client: opens when a torrent is chosen
  // in the list and shows what is going on with it. Tabs: overview, peers, files, speed.
  import { fly } from 'svelte/transition';
  import { t } from '../i18n';
  import { torrents } from '../stores/torrents.svelte';
  import { motion } from '../util/motion';
  import { isHeld } from '../util/torrents';
  import Icon from './Icon.svelte';
  import SpeedChart from './SpeedChart.svelte';
  import TorrentFilesTab from './TorrentFilesTab.svelte';
  import TorrentOverview from './TorrentOverview.svelte';
  import TorrentPeersTab from './TorrentPeersTab.svelte';
  import TorrentStatusBadge from './TorrentStatusBadge.svelte';

  const TABS = ['overview', 'peers', 'files', 'speed'] as const;
  type Tab = (typeof TABS)[number];

  let tab = $state<Tab>('overview');
  const torrent = $derived(torrents.selected);
  const history = $derived(torrent ? (torrents.history[torrent.id] ?? []) : []);

  /** Escape closes the panel, unless a dialog is open (it takes Escape for itself). */
  function onEscape(e: KeyboardEvent): void {
    if (e.key === 'Escape' && torrent && !document.querySelector('dialog[open]')) {
      torrents.closeDetails();
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    const i = TABS.indexOf(tab);
    if (e.key === 'ArrowRight') tab = TABS[(i + 1) % TABS.length] ?? tab;
    else if (e.key === 'ArrowLeft') tab = TABS[(i + TABS.length - 1) % TABS.length] ?? tab;
    else return;
    e.preventDefault();
    document.getElementById(`torrent-tab-${tab}`)?.focus();
  }
</script>

<svelte:window onkeydown={onEscape} />

{#if torrent}
  <section
    transition:fly={{ y: 40, duration: motion(180) }}
    class="fixed inset-x-0 bottom-0 z-20 flex h-[min(24rem,55dvh)] flex-col border-t border-border bg-surface shadow-[0_-8px_24px_-12px_rgb(0_0_0/0.25)] lg:left-(--sidebar-w)"
    aria-label={t('torrent.panel.label')}
    data-part="torrent-panel"
  >
    <header class="flex items-center gap-2 border-b border-border px-3 pt-2 sm:px-4">
      <div class="flex min-w-0 flex-1 flex-col gap-1 sm:flex-row sm:items-center sm:gap-3">
        <h2 class="min-w-0 truncate font-semibold" title={torrent.name}>{torrent.name}</h2>
        <span class="hidden sm:inline"><TorrentStatusBadge status={torrent.status} held={isHeld(torrent)} /></span>
      </div>
      <button
        type="button"
        class="btn icon ghost"
        onclick={() => torrents.closeDetails()}
        aria-label={t('torrent.panel.close')}
        title={t('torrent.panel.close')}
        data-part="torrent-panel-close"
      >
        <Icon name="x" />
      </button>
    </header>
    <div
      class="flex gap-1 overflow-x-auto border-b border-border px-2 sm:px-3"
      role="tablist"
      aria-label={t('torrent.panel.label')}
      tabindex="-1"
      onkeydown={onKeydown}
    >
      {#each TABS as id (id)}
        <button
          type="button"
          role="tab"
          id="torrent-tab-{id}"
          aria-selected={tab === id}
          aria-controls="torrent-tabpanel"
          tabindex={tab === id ? 0 : -1}
          class={[
            '-mb-px whitespace-nowrap border-b-2 px-3 py-2 text-sm font-medium transition-colors',
            tab === id ? 'border-accent text-accent' : 'border-transparent text-muted hover:text-text',
          ]}
          onclick={() => (tab = id)}
          data-tab={id}
        >
          {t(`torrent.tab.${id}`)}
        </button>
      {/each}
    </div>
    <div
      id="torrent-tabpanel"
      role="tabpanel"
      aria-labelledby="torrent-tab-{tab}"
      tabindex="0"
      class="min-h-0 flex-1 overflow-y-auto px-3 py-3 pb-[calc(0.75rem+env(safe-area-inset-bottom))] sm:px-4"
    >
      {#if tab === 'overview'}
        <TorrentOverview {torrent} />
      {:else if tab === 'peers'}
        <TorrentPeersTab {torrent} />
      {:else if tab === 'files'}
        <TorrentFilesTab {torrent} />
      {:else}
        <SpeedChart {history} />
      {/if}
    </div>
  </section>
{/if}
