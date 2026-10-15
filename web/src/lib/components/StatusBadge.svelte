<script lang="ts">
  import type { Status } from '../api/types';
  import { t } from '../i18n';

  let { status }: { status: Status } = $props();

  const STYLE: Record<Status, string> = {
    queued: 'bg-surface-2 text-muted',
    downloading: 'bg-accent-soft text-accent',
    paused: 'bg-warning-soft text-warning',
    completed: 'bg-success-soft text-success',
    failed: 'bg-danger-soft text-danger',
    cancelled: 'bg-danger-soft text-danger',
  };
</script>

<span
  data-status={status}
  class={[
    'inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap',
    STYLE[status],
  ]}
>
  <span class={['size-1.5 rounded-full bg-current', status === 'downloading' && 'animate-pulse']}></span>
  {t(`status.${status}`)}
</span>
