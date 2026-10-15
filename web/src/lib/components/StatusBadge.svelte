<script lang="ts">
  import type { Status } from '../api/types';
  import { t } from '../i18n';

  let { status }: { status: Status } = $props();
</script>

<span class="badge {status}">{t(`status.${status}`)}</span>

<style>
  .badge {
    display: inline-block;
    padding: 0.1rem 0.55rem;
    border-radius: 999px;
    font-size: 0.78rem;
    font-weight: 600;
    white-space: nowrap;
    background: var(--surface-2);
    color: var(--muted);
  }
  .downloading {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .completed {
    background: var(--success-soft);
    color: var(--success);
  }
  .paused {
    background: var(--warning-soft);
    color: var(--warning);
  }
  .failed,
  .cancelled {
    background: var(--danger-soft);
    color: var(--danger);
  }
</style>
