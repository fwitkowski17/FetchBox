<script lang="ts">
  import type { Status } from '../api/types';

  interface Props {
    /** 0..1, or `null` when the total size is unknown. */
    value: number | null;
    status: Status;
    label: string;
  }

  let { value, status, label }: Props = $props();

  const FILL: Record<Status, string> = {
    queued: 'bg-muted',
    downloading: 'bg-accent',
    paused: 'bg-warning',
    completed: 'bg-success',
    failed: 'bg-danger',
    cancelled: 'bg-danger',
  };

  const indeterminate = $derived(value === null && status === 'downloading');
  const percent = $derived(value === null ? 0 : Math.round(value * 1000) / 10);
</script>

<div
  class="h-1.5 overflow-hidden rounded-full bg-surface-2"
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={indeterminate || value === null ? undefined : Math.floor(percent)}
>
  <div
    class={[
      'h-full rounded-full transition-[width] duration-500 ease-out',
      FILL[status],
      indeterminate && 'w-1/3 animate-slide',
    ]}
    style:width={indeterminate ? undefined : `${percent}%`}
  ></div>
</div>
