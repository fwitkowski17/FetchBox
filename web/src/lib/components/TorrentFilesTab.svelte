<script lang="ts">
  // The files of a torrent and how much of each is on disk. Asked every few seconds while this tab
  // is visible (progress changes, the list itself does not).
  import { api } from '../api/client';
  import type { Torrent, TorrentFile } from '../api/types';
  import { i18n, t } from '../i18n';
  import ProgressBar from './ProgressBar.svelte';

  let { torrent }: { torrent: Torrent } = $props();

  const POLL_MS = 3000;
  let files = $state.raw<TorrentFile[] | null>(null);
  let failed = $state(false);
  const id = $derived(torrent.id);

  $effect(() => {
    const mine = id;
    const ctl = new AbortController();
    files = null;
    failed = false;
    async function load(): Promise<void> {
      try {
        const answer = await api.torrentFiles(mine, ctl.signal);
        if (ctl.signal.aborted) return;
        files = answer;
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

  function fraction(f: TorrentFile): number {
    return f.size === 0 ? 1 : Math.min(1, f.downloaded / f.size);
  }
</script>

<div data-part="files">
  {#if files && files.length > 0}
    <ul class="grid gap-2">
      {#each files as f (f.path)}
        <li class="grid gap-1" data-part="file">
          <div class="flex items-baseline justify-between gap-3 text-sm">
            <span class="min-w-0 truncate" title={f.path}>{f.path}</span>
            <span class="shrink-0 text-muted tabular-nums">
              {i18n.bytes(f.size)} · {Math.floor(fraction(f) * 100)}%
            </span>
          </div>
          <ProgressBar value={fraction(f)} status={fraction(f) >= 1 ? 'completed' : 'downloading'} label={f.path} />
        </li>
      {/each}
    </ul>
  {:else if files}
    <p class="py-6 text-center text-muted">{t('torrent.files.empty')}</p>
  {:else if failed}
    <p class="py-6 text-center text-muted" role="alert">{t('torrent.files.loadFailed')}</p>
  {:else}
    <p class="py-6 text-center text-muted" role="status">{t('common.loading')}</p>
  {/if}
</div>
