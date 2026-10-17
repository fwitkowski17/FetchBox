<script lang="ts">
  import type { TorrentStatus } from '../api/types';
  import { t } from '../i18n';

  let { status }: { status: TorrentStatus } = $props();

  const STYLE: Record<TorrentStatus, string> = {
    metadata: 'bg-surface-2 text-muted',
    checking: 'bg-surface-2 text-muted',
    connecting: 'bg-warning-soft text-warning',
    downloading: 'bg-accent-soft text-accent',
    seeding: 'bg-success-soft text-success',
    paused: 'bg-surface-2 text-muted',
    finished: 'bg-success-soft text-success',
    error: 'bg-danger-soft text-danger',
  };
  /** The statuses in which something is going on: their dot pulses. */
  const BUSY: readonly TorrentStatus[] = ['metadata', 'checking', 'connecting', 'downloading', 'seeding'];
</script>

<span
  data-status={status}
  class={[
    'inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap',
    STYLE[status],
  ]}
>
  <span class={['size-1.5 rounded-full bg-current', BUSY.includes(status) && 'animate-pulse']}></span>
  {t(`torrent.status.${status}`)}
</span>
