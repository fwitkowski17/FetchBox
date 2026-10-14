<script lang="ts">
  import type { Status } from '../api/types';

  interface Props {
    /** 0..1, or `null` when the total size is unknown. */
    value: number | null;
    status: Status;
    label: string;
  }

  let { value, status, label }: Props = $props();

  const indeterminate = $derived(value === null && status === 'downloading');
  const percent = $derived(value === null ? 0 : Math.round(value * 1000) / 10);
</script>

<div
  class="bar {status}"
  class:indeterminate
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={indeterminate || value === null ? undefined : Math.floor(percent)}
>
  <div class="fill" style:width={indeterminate ? undefined : `${percent}%`}></div>
</div>

<style>
  .bar {
    --fill: var(--accent);
    height: 0.6rem;
    overflow: hidden;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--border);
  }

  .bar.completed {
    --fill: var(--success);
  }
  .bar.paused {
    --fill: var(--warning);
  }
  .bar.failed,
  .bar.cancelled {
    --fill: var(--danger);
  }
  .bar.queued {
    --fill: var(--muted);
  }

  .fill {
    height: 100%;
    border-radius: inherit;
    background: var(--fill);
    transition: width 0.4s ease;
  }

  .indeterminate .fill {
    width: 35%;
    animation: slide 1.4s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }
</style>
