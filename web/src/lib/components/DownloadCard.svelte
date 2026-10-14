<script lang="ts">
  import type { Category, Download } from '../api/types';
  import { CATEGORIES, CATEGORY_LABELS } from '../api/types';
  import { confirmAction } from '../stores/confirm.svelte';
  import { downloads } from '../stores/downloads.svelte';
  import { speedOf } from '../util/downloads';
  import { etaSeconds, formatBytes, formatDuration, formatPercent, formatSpeed, fraction, hostOf } from '../util/format';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';
  import ProgressBar from './ProgressBar.svelte';
  import StatusBadge from './StatusBadge.svelte';

  let { download }: { download: Download } = $props();

  let busy = $state(false);

  const d = $derived(download);
  const frac = $derived(fraction(d.downloaded, d.size));
  const speed = $derived(speedOf(d));
  const eta = $derived(etaSeconds(d.downloaded, d.size, speed));
  const live = $derived(d.status === 'downloading');
  const unfinished = $derived(d.status === 'queued' || d.status === 'downloading' || d.status === 'paused');
  const folder = $derived(d.subdir ? `${d.subdir}/` : '');

  /** The one big button that matters for this status. */
  const primary = $derived.by(() => {
    switch (d.status) {
      case 'queued':
      case 'downloading':
        return { label: 'Pause', icon: 'pause', run: () => downloads.pause(d.id) } as const;
      case 'paused':
        return { label: 'Resume', icon: 'play', run: () => downloads.resume(d.id) } as const;
      case 'failed':
        return { label: 'Retry', icon: 'retry', run: () => downloads.resume(d.id) } as const;
      default:
        return null;
    }
  });

  async function act(run: () => Promise<boolean>): Promise<void> {
    busy = true;
    try {
      await run();
    } finally {
      busy = false;
    }
  }

  async function cancel(): Promise<void> {
    const ok = await confirmAction({
      title: 'Cancel this download?',
      message: `“${d.filename}” will stop and the partly downloaded file will be deleted.`,
      confirmLabel: 'Cancel download',
      danger: true,
    });
    if (ok) await act(() => downloads.cancel(d.id));
  }

  const remove = () => act(() => downloads.remove(d.id));

  function changeCategory(e: Event): void {
    const value = (e.currentTarget as HTMLSelectElement).value as Category;
    void act(() => downloads.setCategory(d.id, value));
  }
</script>

<article class="card" class:done={d.status === 'completed'} aria-label={d.filename}>
  <div class="top">
    <CategoryIcon category={d.category} tile />
    <div class="title">
      <h3 title={d.filename}>{d.filename}</h3>
      <p class="meta">
        <span class="host">{hostOf(d.url)}</span>
        {#if folder}<span class="folder"><Icon name="folder" size={13} />{folder}</span>{/if}
      </p>
    </div>
    <StatusBadge status={d.status} />
  </div>

  {#if d.status !== 'cancelled'}
    <ProgressBar value={d.status === 'completed' ? 1 : frac} status={d.status} label={`Progress of ${d.filename}`} />
  {/if}

  <p class="stats">
    {#if d.status === 'queued'}
      Waiting for a free slot{d.size != null ? ` · ${formatBytes(d.size)}` : ''}
    {:else if d.status === 'completed'}
      {formatBytes(d.size ?? d.downloaded)}
    {:else if d.status === 'cancelled'}
      Cancelled
    {:else}
      <span class="num">
        {formatBytes(d.downloaded)}{d.size != null ? ` of ${formatBytes(d.size)}` : ''}
        {#if frac !== null}<strong>{formatPercent(frac)}</strong>{/if}
      </span>
      {#if live}
        <span class="num">{formatSpeed(speed)}</span>
        {#if eta !== null}<span class="num">{formatDuration(eta)} left</span>{/if}
      {/if}
    {/if}
  </p>

  {#if d.status === 'failed' && d.error}
    <p class="error" role="alert"><Icon name="alert" size={16} />{d.error}</p>
  {/if}
  {#if unfinished && !d.resumable}
    <p class="note">This server does not allow resuming, so pausing restarts the download.</p>
  {/if}

  <div class="actions">
    {#if primary}
      <button class="btn primary grow" disabled={busy} onclick={() => act(primary.run)}>
        <Icon name={primary.icon} size={18} />{primary.label}
      </button>
    {/if}

    <label class="catpick">
      <span class="sr-only">Category of {d.filename}</span>
      <select class="select" value={d.category} disabled={busy} onchange={changeCategory}>
        {#each CATEGORIES as c (c)}
          <option value={c}>{CATEGORY_LABELS[c]}</option>
        {/each}
      </select>
    </label>

    {#if unfinished}
      <button class="btn icon" disabled={busy} aria-label={`Cancel ${d.filename}`} title="Cancel" onclick={cancel}>
        <Icon name="x" />
      </button>
    {:else}
      <button
        class="btn icon"
        disabled={busy}
        aria-label={`Remove ${d.filename} from the list`}
        title={d.status === 'completed' ? 'Remove from list (the file stays on disk)' : 'Remove from list'}
        onclick={remove}
      >
        <Icon name="trash" />
      </button>
    {/if}
  </div>
</article>

<style>
  .card {
    display: grid;
    gap: 0.7rem;
    padding: 0.9rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  .top {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .title {
    flex: 1;
    min-width: 0;
  }

  h3 {
    font-size: 1rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 0.6rem;
    color: var(--muted);
    font-size: 0.82rem;
  }

  .host {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .folder {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }

  .stats {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 0.9rem;
    color: var(--muted);
    font-size: 0.88rem;
  }

  .num {
    font-variant-numeric: tabular-nums;
  }

  .stats strong {
    color: var(--text);
    margin-left: 0.25rem;
  }

  .error {
    display: flex;
    gap: 0.4rem;
    align-items: flex-start;
    padding: 0.5rem 0.65rem;
    border-radius: var(--radius-sm);
    background: var(--danger-soft);
    color: var(--danger);
    font-size: 0.88rem;
    overflow-wrap: anywhere;
  }

  .error :global(svg) {
    margin-top: 0.1rem;
  }

  .note {
    color: var(--warning);
    font-size: 0.82rem;
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  .grow {
    flex: 1;
  }

  .catpick {
    flex: 0 1 10rem;
    min-width: 0;
  }

  /* Without the big button (finished downloads) the remove button sits at the far end. */
  .actions > .btn.icon:last-child {
    margin-left: auto;
  }

  .select {
    padding-right: 0.4rem;
  }
</style>
