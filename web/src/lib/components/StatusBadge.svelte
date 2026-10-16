<script lang="ts">
  import type { Hold, Status } from '../api/types';
  import { t } from '../i18n';

  /** `waiting`: paused because the disk is full (shown as "Waiting for space"). `hold`: queued
   *  on purpose until the download hours open or a start time comes. */
  let {
    status,
    waiting = false,
    hold = null,
  }: { status: Status; waiting?: boolean; hold?: Hold | null | undefined } = $props();

  const STYLE: Record<Status, string> = {
    queued: 'bg-surface-2 text-muted',
    downloading: 'bg-accent-soft text-accent',
    paused: 'bg-warning-soft text-warning',
    verifying: 'bg-accent-soft text-accent',
    completed: 'bg-success-soft text-success',
    failed: 'bg-danger-soft text-danger',
    cancelled: 'bg-danger-soft text-danger',
  };
</script>

<span
  data-status={status}
  data-waiting={waiting ? 'space' : (hold ?? undefined)}
  class={[
    'inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap',
    STYLE[status],
  ]}
>
  <span class={['size-1.5 rounded-full bg-current', (status === 'downloading' || status === 'verifying') && 'animate-pulse']}></span>
  {#if waiting}
    {t('status.waitingSpace')}
  {:else if hold}
    {t(hold === 'time' ? 'status.scheduled' : 'status.waitingHours')}
  {:else}
    {t(`status.${status}`)}
  {/if}
</span>
