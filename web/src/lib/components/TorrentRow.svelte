<script lang="ts">
  // One torrent in the list: name, status, a progress bar and the numbers a BitTorrent client shows.
  // The whole row opens the details panel (the name is the button, stretched over the row); the
  // action buttons sit above it.
  import type { Torrent } from '../api/types';
  import { i18n, t } from '../i18n';
  import { deleteTorrentDialog } from '../stores/deleteTorrentDialog.svelte';
  import { torrents } from '../stores/torrents.svelte';
  import { primaryAction, progressOf, ratioOf } from '../util/torrents';
  import Icon from './Icon.svelte';
  import ProgressBar from './ProgressBar.svelte';
  import TorrentStatusBadge from './TorrentStatusBadge.svelte';

  let { torrent }: { torrent: Torrent } = $props();

  const selected = $derived(torrents.selectedId === torrent.id);
  const progress = $derived(progressOf(torrent));
  const percent = $derived(progress === null ? null : Math.floor(progress * 1000) / 10);
  const action = $derived(primaryAction(torrent));
  const ratio = $derived(ratioOf(torrent));
  const moving = $derived(torrent.status === 'downloading' || torrent.status === 'seeding');

  /** The bar's colours come from the downloads' statuses. */
  const barStatus = $derived(
    torrent.status === 'error'
      ? 'failed'
      : torrent.status === 'seeding' || torrent.status === 'finished'
        ? 'completed'
        : torrent.status === 'paused'
          ? 'paused'
          : torrent.status === 'downloading'
            ? 'downloading'
            : 'queued',
  );

  function primary(): void {
    if (action === 'pause') void torrents.pause(torrent.id);
    else void torrents.resume(torrent.id);
  }
</script>

<article
  class={[
    'relative grid gap-2 rounded-xl border bg-surface p-3 shadow-card transition-colors hover:border-accent/60',
    selected ? 'border-accent ring-1 ring-accent' : 'border-border',
  ]}
  data-part="torrent"
  data-status={torrent.status}
  data-selected={selected ? '' : undefined}
>
  <div class="flex items-center gap-2">
    <h3 class="min-w-0 flex-1 font-semibold">
      <!-- Stretched over the whole row; the buttons below are raised above it. -->
      <button
        type="button"
        class="block w-full truncate text-left after:absolute after:inset-0 after:rounded-xl focus-visible:outline-none focus-visible:after:ring-2 focus-visible:after:ring-accent"
        aria-pressed={selected}
        aria-label={t('torrent.select', { name: torrent.name })}
        title={torrent.name}
        onclick={() => torrents.toggleSelected(torrent.id)}
        data-part="torrent-open"
      >
        {torrent.name}
      </button>
    </h3>
    <TorrentStatusBadge status={torrent.status} />
    <button
      type="button"
      class="btn icon ghost relative z-10"
      onclick={primary}
      aria-label={t(`torrent.${action}`)}
      title={t(`torrent.${action}`)}
      data-part="torrent-primary"
      data-action={action}
    >
      <Icon name={action === 'pause' ? 'pause' : action === 'retry' ? 'retry' : 'play'} />
    </button>
    <button
      type="button"
      class="btn icon ghost danger relative z-10"
      onclick={() => deleteTorrentDialog.show(torrent.id)}
      aria-label={t('torrent.remove')}
      title={t('torrent.remove')}
      data-part="torrent-remove"
    >
      <Icon name="trash" />
    </button>
  </div>

  {#if torrent.status === 'metadata'}
    <p class="text-sm text-muted">{t('torrent.metadataPending')}</p>
  {:else}
    <ProgressBar value={progress} status={barStatus} label={torrent.name} />
  {/if}

  <p class="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-muted tabular-nums" data-part="torrent-stats">
    {#if torrent.size !== null && torrent.size !== undefined}
      <span title={t('torrent.field.size')}>
        {torrent.status === 'seeding' || torrent.status === 'finished'
          ? i18n.bytes(torrent.size)
          : `${i18n.bytes(torrent.downloaded)} / ${i18n.bytes(torrent.size)}`}
        {#if percent !== null}<span class="font-medium text-text">· {percent}%</span>{/if}
      </span>
    {/if}
    {#if moving || torrent.down_speed > 0}
      <span class="inline-flex items-center gap-0.5 text-accent" title={t('torrent.field.downSpeed')}>
        <Icon name="down" size={14} />{i18n.speed(torrent.down_speed)}
      </span>
    {/if}
    {#if moving || torrent.up_speed > 0}
      <span class="inline-flex items-center gap-0.5 text-success" title={t('torrent.field.upSpeed')}>
        <Icon name="up" size={14} />{i18n.speed(torrent.up_speed)}
      </span>
    {/if}
    {#if torrent.status !== 'paused' && torrent.status !== 'finished' && torrent.status !== 'error'}
      <span class="inline-flex items-center gap-1" title={t('torrent.field.peers')}>
        <Icon name="users" size={14} />{t('torrent.peersCount', {
          connected: torrent.peers_connected,
          known: torrent.peers_known,
        })}
      </span>
    {/if}
    {#if torrent.eta !== null && torrent.eta !== undefined && torrent.status === 'downloading'}
      <span title={t('torrent.field.eta')}>{i18n.duration(torrent.eta)}</span>
    {/if}
    {#if ratio !== null}
      <span title={t('torrent.field.ratio')}>{t('torrent.ratioShort', { ratio: ratio.toFixed(2) })}</span>
    {/if}
  </p>

  {#if torrent.error}
    <p class="text-sm text-danger [overflow-wrap:anywhere]" role="alert">{torrent.error}</p>
  {/if}
</article>
