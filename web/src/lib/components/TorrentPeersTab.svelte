<script lang="ts">
  // The peers of a torrent: who we are connected to, with what client, how fast. Not part of the
  // event stream (a busy swarm has hundreds): asked every two seconds while this tab is visible.
  import { api } from '../api/client';
  import type { PeerState, Torrent, TorrentPeers } from '../api/types';
  import { i18n, t } from '../i18n';
  import { isActive } from '../util/torrents';

  let { torrent }: { torrent: Torrent } = $props();

  const POLL_MS = 2000;
  let data = $state.raw<TorrentPeers | null>(null);
  let failed = $state(false);
  const id = $derived(torrent.id);

  $effect(() => {
    const mine = id;
    const ctl = new AbortController();
    data = null;
    failed = false;
    async function load(): Promise<void> {
      try {
        const answer = await api.torrentPeers(mine, ctl.signal);
        if (ctl.signal.aborted) return;
        data = answer;
        failed = false;
      } catch {
        if (!ctl.signal.aborted) failed = true;
      }
    }
    void load();
    const timer = setInterval(() => {
      if (!document.hidden) void load();
    }, POLL_MS);
    return () => {
      ctl.abort();
      clearInterval(timer);
    };
  });

  const STATE_STYLE: Record<PeerState, string> = {
    connected: 'text-success',
    connecting: 'text-warning',
    queued: 'text-muted',
    idle: 'text-muted',
    dead: 'text-danger',
  };
  const TH = 'px-2 py-1.5 text-left text-xs font-semibold tracking-wide text-muted uppercase';
  const TD = 'px-2 py-1.5 whitespace-nowrap';
</script>

<div class="grid gap-3" data-part="peers">
  {#if data}
    <p class="flex flex-wrap gap-x-4 gap-y-1 text-sm tabular-nums" data-part="peers-summary">
      <span class="font-medium text-success">{t('torrent.peers.connected', { n: data.connected })}</span>
      <span class="text-warning">{t('torrent.peers.connecting', { n: data.connecting })}</span>
      <span class="text-muted">{t('torrent.peers.known', { n: data.known })}</span>
    </p>
    {#if data.peers.length === 0}
      <p class="py-6 text-center text-muted">
        {isActive(torrent) ? t('torrent.peers.empty') : t('torrent.peers.stopped')}
      </p>
    {:else}
      <div class="overflow-x-auto">
        <table class="w-full border-collapse text-sm tabular-nums">
          <thead>
            <tr class="border-b border-border">
              <th class={TH}>{t('torrent.peers.address')}</th>
              <th class="{TH} hidden sm:table-cell">{t('torrent.peers.client')}</th>
              <th class="{TH} hidden md:table-cell">{t('torrent.peers.transport')}</th>
              <th class="{TH} text-right">{t('torrent.peers.down')}</th>
              <th class="{TH} text-right">{t('torrent.peers.up')}</th>
              <th class="{TH} hidden text-right md:table-cell">{t('torrent.peers.received')}</th>
              <th class="{TH} hidden text-right md:table-cell">{t('torrent.peers.sent')}</th>
              <th class={TH}>{t('torrent.peers.state')}</th>
            </tr>
          </thead>
          <tbody>
            {#each data.peers as p (p.address)}
              <tr class="border-b border-border/60 last:border-0" data-part="peer" data-state={p.state}>
                <td class="{TD} font-mono text-xs">{p.address}</td>
                <td class="{TD} hidden max-w-48 truncate sm:table-cell" title={p.client ?? ''}>{p.client ?? '—'}</td>
                <td class="{TD} hidden uppercase md:table-cell">{p.transport ?? '—'}</td>
                <td class="{TD} text-right {p.down_speed > 0 ? 'text-accent' : 'text-muted'}">{i18n.speed(p.down_speed)}</td>
                <td class="{TD} text-right {p.up_speed > 0 ? 'text-success' : 'text-muted'}">{i18n.speed(p.up_speed)}</td>
                <td class="{TD} hidden text-right md:table-cell">{i18n.bytes(p.downloaded)}</td>
                <td class="{TD} hidden text-right md:table-cell">{i18n.bytes(p.uploaded)}</td>
                <td class="{TD} {STATE_STYLE[p.state]}">{t(`torrent.peer.${p.state}`)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if data.truncated}
        <p class="hint">{t('torrent.peers.truncated', { n: data.peers.length })}</p>
      {/if}
    {/if}
    <p class="hint">{t('torrent.peers.note')}</p>
  {:else if failed}
    <p class="py-6 text-center text-muted" role="alert">{t('torrent.peers.loadFailed')}</p>
  {:else}
    <p class="py-6 text-center text-muted" role="status">{t('common.loading')}</p>
  {/if}
</div>
