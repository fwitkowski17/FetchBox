<script lang="ts">
  import type { Download } from '../api/types';
  import { i18n, t } from '../i18n';
  import { deleteDialog } from '../stores/deleteDialog.svelte';
  import { downloads } from '../stores/downloads.svelte';
  import { confirmCancel, primaryUi, runPrimary } from '../stores/downloadActions';
  import { selection } from '../stores/selection.svelte';
  import { algoLabel } from '../util/checksum';
  import { isHeld, isVerifyFailed, primaryAction, speedOf } from '../util/downloads';
  import { folderOf } from '../util/disks';
  import { etaSeconds, formatPercent, fraction, hostOf } from '../util/format';
  import { formatWhen } from '../util/schedule';
  import CardMenu from './CardMenu.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';
  import ProgressBar from './ProgressBar.svelte';
  import SpaceGauge from './SpaceGauge.svelte';
  import StatusBadge from './StatusBadge.svelte';

  let { download }: { download: Download } = $props();

  let busy = $state(false);

  const d = $derived(download);
  const frac = $derived(fraction(d.downloaded, d.size));
  const speed = $derived(speedOf(d));
  const eta = $derived(etaSeconds(d.downloaded, d.size, speed));
  const live = $derived(d.status === 'downloading');
  const selecting = $derived(selection.active);
  const selected = $derived(selection.has(d.id));
  /** Held back because the disk is full; the gauge below says by how much. */
  const heldForSpace = $derived(d.status === 'paused' && d.waiting_space === true && d.size != null);
  const unfinished = $derived(d.status === 'queued' || d.status === 'downloading' || d.status === 'paused');
  const folder = $derived(folderOf(d));
  /** Queued on purpose (download hours, or a start time): it starts by itself, or now on request. */
  const held = $derived(isHeld(d));
  /** The file is complete and its checksum is being computed. */
  const verifying = $derived(d.status === 'verifying');
  /** Completed, and the file matched its checksum. */
  const verified = $derived(d.status === 'completed' && d.verified === true && d.checksum != null);
  /** The checksum did not match (or the file could not be read): the user decides what to do. */
  const verifyFailed = $derived(isVerifyFailed(d));
  const sumLabel = $derived(d.checksum ? algoLabel(d.checksum.algo) : '');
  /** Kept by the user although the checksum did not match: completed, with a warning beside it. */
  const keptMismatch = $derived(d.status === 'completed' && d.verify_failed === true);
  const keptText = $derived(
    d.checksum?.actual
      ? t('card.keptMismatch', { algo: sumLabel, expected: d.checksum.expected, actual: d.checksum.actual })
      : t('card.keptUnverified'),
  );

  /** The one big button that matters for this status. */
  const primary = $derived.by(() => {
    const kind = primaryAction(d);
    return kind ? { ...primaryUi(kind), run: () => runPrimary(kind, d.id) } : null;
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
    if (await confirmCancel(d)) await act(() => downloads.cancel(d.id));
  }

  const remove = () => act(() => downloads.remove(d.id));
  const keepFile = () => act(() => downloads.keep(d.id));
</script>

<!-- In selection mode the whole card is the target; the checkbox is for keyboards and screen readers. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<article
  class={[
    'grid gap-3 rounded-2xl border bg-surface p-4 shadow-card transition-colors',
    selected ? 'border-accent bg-accent-soft' : 'border-border',
    selecting && 'cursor-pointer',
  ]}
  aria-label={d.filename}
  data-selected={selecting ? String(selected) : undefined}
  onclick={(e) => {
    if (selecting && !(e.target instanceof HTMLInputElement)) selection.toggle(d.id);
  }}
>
  <div class="flex items-center gap-3">
    {#if selecting}
      <input
        type="checkbox"
        class="size-5 shrink-0 accent-[var(--accent)]"
        checked={selected}
        onchange={() => selection.toggle(d.id)}
        aria-label={t('selection.card', { name: d.filename })}
        data-part="select"
      />
    {/if}
    <CategoryIcon category={d.category} tile />
    <div class="min-w-0 flex-1">
      <h3 class="truncate font-semibold tracking-tight" title={d.filename}>{d.filename}</h3>
      <p class="flex flex-wrap items-center gap-x-3 text-sm text-muted">
        <span class="max-w-full truncate" title={d.username ? t('card.login', { user: d.username }) : undefined}>
          {#if d.source_kind === 'ftp' || d.source_kind === 'sftp'}<span class="font-semibold uppercase">{d.source_kind}</span>{/if}
          {d.username ? `${d.username}@` : ''}{hostOf(d.url)}
        </span>
        {#if folder}
          <span class="inline-flex items-center gap-1"><Icon name="folder" size={13} />{folder}</span>
        {/if}
      </p>
    </div>
    {#if keptMismatch}
      <span class="inline-flex shrink-0 text-warning" role="img" title={keptText} aria-label={keptText} data-part="kept-warning">
        <Icon name="alert" size={20} />
      </span>
    {/if}
    <StatusBadge status={d.status} waiting={heldForSpace} hold={held ? d.hold : null} />
  </div>

  {#if heldForSpace && d.size != null}
    <SpaceGauge size={d.size} downloaded={d.downloaded} />
  {:else if d.status !== 'cancelled'}
    <ProgressBar
      value={d.status === 'completed' ? 1 : frac}
      status={d.status}
      label={t('card.progressOf', { name: d.filename })}
    />
  {/if}

  {#if !heldForSpace}
    <p data-part="stats" class="flex flex-wrap gap-x-4 gap-y-0.5 text-sm text-muted tabular-nums">
      {#if d.status === 'queued' && held}
        <span data-part="starts-at">
          {d.starts_at != null
            ? t('card.startsAt', { when: formatWhen(d.starts_at, i18n.locale) })
            : t('card.waitingHours')}{d.size != null ? ` · ${i18n.bytes(d.size)}` : ''}
        </span>
        {#if d.speed_limit}<span>{t('card.limit', { speed: i18n.speed(d.speed_limit) })}</span>{/if}
      {:else if d.status === 'queued'}
        <span>{t('card.waiting')}{d.size != null ? ` · ${i18n.bytes(d.size)}` : ''}</span>
        {#if d.speed_limit}<span>{t('card.limit', { speed: i18n.speed(d.speed_limit) })}</span>{/if}
      {:else if verifying}
        <span>
          {t('card.verifying')}
          {#if frac !== null}<strong class="ml-1 font-semibold text-text">{formatPercent(frac)}</strong>{/if}
        </span>
        {#if speed > 0}<span>{i18n.speed(speed)}</span>{/if}
      {:else if d.status === 'completed'}
        <span>{i18n.bytes(d.size ?? d.downloaded)}</span>
        {#if verified}
          <span class="inline-flex items-center gap-1 text-success" data-part="verified">
            <Icon name="checkCircle" size={15} />{t('card.verified', { algo: sumLabel })}
          </span>
        {/if}
      {:else if d.status === 'cancelled'}
        <span>{t('card.cancelled')}</span>
      {:else}
        <span>
          {d.size != null
            ? t('card.ofTotal', { done: i18n.bytes(d.downloaded), total: i18n.bytes(d.size) })
            : i18n.bytes(d.downloaded)}
          {#if frac !== null}<strong class="ml-1 font-semibold text-text">{formatPercent(frac)}</strong>{/if}
        </span>
        {#if live}
          <span>{i18n.speed(speed)}</span>
          {#if eta !== null}<span>{t('card.eta', { time: i18n.duration(eta) })}</span>{/if}
        {/if}
        {#if d.speed_limit && d.status !== 'failed'}
          <span>{t('card.limit', { speed: i18n.speed(d.speed_limit) })}</span>
        {/if}
      {/if}
    </p>
  {/if}

  {#if verifyFailed && d.checksum}
    <div
      class="grid gap-2 rounded-lg bg-danger-soft px-3 py-2 text-sm text-danger"
      role="alert"
      data-part="verify-failed"
    >
      <p class="flex items-start gap-2 font-semibold">
        <span class="mt-0.5"><Icon name="alert" size={16} /></span>
        {d.checksum.actual ? t('card.verifyFailed') : t('card.verifyUnreadable')}
      </p>
      {#if d.checksum.actual}
        <dl class="grid gap-0.5 font-mono text-xs [overflow-wrap:anywhere]">
          <dt class="font-sans font-semibold">{t('card.expected', { algo: sumLabel })}</dt>
          <dd data-part="expected">{d.checksum.expected}</dd>
          <dt class="mt-1 font-sans font-semibold">{t('card.actual')}</dt>
          <dd data-part="actual">{d.checksum.actual}</dd>
        </dl>
      {:else if d.error}
        <p class="[overflow-wrap:anywhere]">{d.error}</p>
      {/if}
      <p class="text-muted">{t('card.verifyFailed.hint')}</p>
    </div>
  {:else if d.status === 'failed' && d.error}
    <p
      class="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-sm text-danger [overflow-wrap:anywhere]"
      role="alert"
    >
      <span class="mt-0.5"><Icon name="alert" size={16} /></span>{d.error}
    </p>
  {/if}
  {#if unfinished && !d.resumable}
    <p class="text-sm text-warning">{t('card.noResume')}</p>
  {/if}

  {#if !selecting}
    <div class="flex flex-wrap items-center gap-2">
      {#if primary}
        <button class="btn primary flex-1" disabled={busy} onclick={() => act(primary.run)}>
          <Icon name={primary.icon} size={18} />{primary.label}
        </button>
      {/if}

      {#if verifyFailed}
        <button class="btn flex-1" disabled={busy} onclick={keepFile} data-part="keep">
          <Icon name="check" size={18} />{t('card.keepFile')}
        </button>
      {/if}

      {#if held}
        <button
          class="btn icon"
          disabled={busy}
          aria-label={t('card.pause')}
          title={t('card.pause')}
          onclick={() => act(() => downloads.pause(d.id))}
        >
          <Icon name="pause" />
        </button>
      {/if}

      <CardMenu download={d} disabled={busy} />

      {#if unfinished}
        <button
          class={['btn icon', !primary && 'ml-auto']}
          disabled={busy}
          aria-label={t('card.cancel.label', { name: d.filename })}
          title={t('card.cancel.title')}
          onclick={cancel}
        >
          <Icon name="x" />
        </button>
      {:else if verifyFailed}
        <!-- The file is still there: the dialog offers to take it with the entry. -->
        <button
          class="btn icon ml-auto"
          disabled={busy}
          aria-label={t('card.remove.label', { name: d.filename })}
          title={t('card.deleteWithFile')}
          onclick={() => deleteDialog.show(d.id)}
          data-part="delete-with-file"
        >
          <Icon name="trash" />
        </button>
      {:else}
        <button
          class="btn icon ml-auto"
          disabled={busy}
          aria-label={t('card.remove.label', { name: d.filename })}
          title={d.status === 'completed' || verifying ? t('card.remove.titleKeepsFile') : t('card.remove.title')}
          onclick={remove}
        >
          <Icon name="trash" />
        </button>
      {/if}
    </div>
  {/if}
</article>
