<script lang="ts">
  // The "general" tab: everything the list row shows, in full, plus the identity of the torrent.
  import type { Torrent } from '../api/types';
  import { i18n, t } from '../i18n';
  import { toast } from '../stores/toast.svelte';
  import { copyText } from '../util/clipboard';
  import { isHeld, progressOf, ratioOf } from '../util/torrents';
  import Icon from './Icon.svelte';
  import ProgressBar from './ProgressBar.svelte';
  import TorrentStatusBadge from './TorrentStatusBadge.svelte';

  let { torrent }: { torrent: Torrent } = $props();

  const progress = $derived(progressOf(torrent));
  const ratio = $derived(ratioOf(torrent));
  const barStatus = $derived(
    torrent.status === 'error' ? 'failed' : torrent.status === 'seeding' || torrent.status === 'finished' ? 'completed' : 'downloading',
  );

  async function copy(text: string): Promise<void> {
    if (await copyText(text)) toast.success(t('common.copied'));
  }

  const LABEL = 'text-xs font-semibold tracking-wide text-muted uppercase';
</script>

<dl class="grid gap-x-6 gap-y-3 sm:grid-cols-2 lg:grid-cols-3" data-part="overview">
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.status')}</dt>
    <dd><TorrentStatusBadge status={torrent.status} held={isHeld(torrent)} /></dd>
  </div>
  <div class="grid gap-1 sm:col-span-1 lg:col-span-2">
    <dt class={LABEL}>{t('torrent.field.progress')}</dt>
    <dd class="grid gap-1">
      {#if progress !== null}
        <ProgressBar value={progress} status={barStatus} label={torrent.name} />
        <span class="text-sm tabular-nums">{(Math.floor(progress * 1000) / 10).toFixed(1)}%</span>
      {:else}
        <span class="text-muted">—</span>
      {/if}
    </dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.size')}</dt>
    <dd class="tabular-nums">{torrent.size !== null && torrent.size !== undefined ? i18n.bytes(torrent.size) : '—'}</dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.downloaded')}</dt>
    <dd class="tabular-nums">{i18n.bytes(torrent.downloaded)}</dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.uploaded')}</dt>
    <dd class="tabular-nums">{i18n.bytes(torrent.uploaded)}</dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.ratio')}</dt>
    <dd class="tabular-nums">{ratio === null ? '—' : ratio.toFixed(2)}</dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.downSpeed')}</dt>
    <dd class="tabular-nums text-accent">{i18n.speed(torrent.down_speed)}</dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.upSpeed')}</dt>
    <dd class="tabular-nums text-success">{i18n.speed(torrent.up_speed)}</dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.eta')}</dt>
    <dd class="tabular-nums">{torrent.eta !== null && torrent.eta !== undefined ? i18n.duration(torrent.eta) : '—'}</dd>
  </div>
  <div class="grid gap-1 sm:col-span-2">
    <dt class={LABEL}>{t('torrent.field.peers')}</dt>
    <dd>
      {t('torrent.field.peersValue', {
        connected: torrent.peers_connected,
        connecting: torrent.peers_connecting,
        known: torrent.peers_known,
      })}
    </dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.files')}</dt>
    <dd class="tabular-nums">{torrent.file_count > 0 ? torrent.file_count : '—'}</dd>
  </div>
  <div class="grid gap-1" data-part="torrent-disk">
    <dt class={LABEL}>{t('torrent.field.disk')}</dt>
    <dd>{torrent.disk === '' ? t('disks.main') : torrent.disk}</dd>
  </div>
  <div class="grid gap-1">
    <dt class={LABEL}>{t('torrent.field.added')}</dt>
    <dd>{i18n.date(torrent.added_at)}</dd>
  </div>
  {#if torrent.finished_at !== null && torrent.finished_at !== undefined}
    <div class="grid gap-1">
      <dt class={LABEL}>{t('torrent.field.finished')}</dt>
      <dd>{i18n.date(torrent.finished_at)}</dd>
    </div>
  {/if}
  <div class="grid gap-1 sm:col-span-2 lg:col-span-3">
    <dt class={LABEL}>{t('torrent.field.hash')}</dt>
    <dd class="flex items-center gap-2">
      <code class="min-w-0 flex-1 font-mono text-sm break-all" data-part="hash">{torrent.info_hash}</code>
      <button class="btn icon ghost shrink-0" type="button" onclick={() => copy(torrent.info_hash)} aria-label={t('torrent.copyHash')} title={t('torrent.copyHash')}>
        <Icon name="copy" size={18} />
      </button>
    </dd>
  </div>
  {#if torrent.magnet}
    {@const magnet = torrent.magnet}
    <div class="grid gap-1 sm:col-span-2 lg:col-span-3">
      <dt class={LABEL}>{t('torrent.field.magnet')}</dt>
      <dd class="flex items-center gap-2">
        <code class="min-w-0 flex-1 truncate font-mono text-sm" title={magnet}>{magnet}</code>
        <button class="btn icon ghost shrink-0" type="button" onclick={() => copy(magnet)} aria-label={t('torrent.copyMagnet')} title={t('torrent.copyMagnet')}>
          <Icon name="copy" size={18} />
        </button>
      </dd>
    </div>
  {/if}
  {#if torrent.error}
    <div class="grid gap-1 sm:col-span-2 lg:col-span-3">
      <dt class={LABEL}>{t('torrent.field.error')}</dt>
      <dd class="text-danger [overflow-wrap:anywhere]">{torrent.error}</dd>
    </div>
  {/if}
</dl>
