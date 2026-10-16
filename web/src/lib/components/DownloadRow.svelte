<script lang="ts">
  // One download as a row of the table view. The same buttons as the card, as icons.
  import type { Download } from '../api/types';
  import { i18n, t } from '../i18n';
  import { deleteDialog } from '../stores/deleteDialog.svelte';
  import { confirmCancel, primaryUi, runPrimary } from '../stores/downloadActions';
  import { downloads } from '../stores/downloads.svelte';
  import { selection } from '../stores/selection.svelte';
  import { algoLabel } from '../util/checksum';
  import { isHeld, isVerifyFailed, primaryAction, speedOf } from '../util/downloads';
  import { etaSeconds, formatPercent, fraction, hostOf } from '../util/format';
  import CardMenu from './CardMenu.svelte';
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
  const selecting = $derived(selection.active);
  const selected = $derived(selection.has(d.id));
  const heldForSpace = $derived(d.status === 'paused' && d.waiting_space === true && d.size != null);
  const unfinished = $derived(d.status === 'queued' || d.status === 'downloading' || d.status === 'paused');
  const held = $derived(isHeld(d));
  const verifyFailed = $derived(isVerifyFailed(d));
  const keptMismatch = $derived(d.status === 'completed' && d.verify_failed === true);
  const sumLabel = $derived(d.checksum ? algoLabel(d.checksum.algo) : '');
  const keptText = $derived(
    d.checksum?.actual
      ? t('card.keptMismatch', { algo: sumLabel, expected: d.checksum.expected, actual: d.checksum.actual })
      : t('card.keptUnverified'),
  );
  const folder = $derived(d.subdir ? `${d.subdir}/` : '');
  const showBar = $derived(d.status !== 'cancelled');
  const barValue = $derived(d.status === 'completed' ? 1 : frac);
  const primary = $derived.by(() => {
    const kind = primaryAction(d);
    return kind ? { ...primaryUi(kind), run: () => runPrimary(kind, d.id) } : null;
  });
  /** What the progress column says: the share done, or nothing while there is nothing to share. */
  const percent = $derived(
    d.status === 'completed' ? formatPercent(1) : frac !== null ? formatPercent(frac) : '',
  );
  /** The short line under the name where the columns are hidden (narrow screens). */
  const brief = $derived(
    [
      percent,
      live ? i18n.speed(speed) : '',
      live && eta !== null ? t('card.eta', { time: i18n.duration(eta) }) : '',
      !live && d.size != null ? i18n.bytes(d.size) : '',
    ]
      .filter(Boolean)
      .join(' · '),
  );

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

  const CELL = 'px-3 py-2.5 align-middle';
</script>

<!-- In selection mode the whole row is the target; the checkbox is for keyboards and screen readers. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<tr
  class={[
    'border-t border-border transition-colors first:border-t-0',
    selected ? 'bg-accent-soft' : 'hover:bg-surface-2/60',
    selecting && 'cursor-pointer',
  ]}
  aria-label={d.filename}
  data-part="row"
  data-selected={selecting ? String(selected) : undefined}
  onclick={(e) => {
    if (selecting && !(e.target instanceof HTMLInputElement)) selection.toggle(d.id);
  }}
>
  {#if selecting}
    <td class="w-10 py-2.5 pr-0 pl-3 align-middle">
      <input
        type="checkbox"
        class="size-5 shrink-0 accent-[var(--accent)]"
        checked={selected}
        onchange={() => selection.toggle(d.id)}
        aria-label={t('selection.card', { name: d.filename })}
        data-part="select"
      />
    </td>
  {/if}

  <td class={[CELL, 'max-w-0 overflow-hidden']}>
    <div class="flex items-center gap-3">
      <CategoryIcon category={d.category} size={18} tile />
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-1.5">
          <span class="truncate font-semibold tracking-tight" title={d.filename}>{d.filename}</span>
          {#if keptMismatch}
            <span class="inline-flex shrink-0 text-warning" role="img" title={keptText} aria-label={keptText} data-part="kept-warning">
              <Icon name="alert" size={16} />
            </span>
          {/if}
        </div>
        <p class="truncate text-sm text-muted" title={d.username ? t('card.login', { user: d.username }) : undefined}>
          {#if d.source_kind === 'ftp' || d.source_kind === 'sftp'}<span class="font-semibold uppercase">{d.source_kind}</span>{/if}
          {d.username ? `${d.username}@` : ''}{hostOf(d.url)}{folder ? ` · ${folder}` : ''}
        </p>
        {#if d.status === 'failed' && d.error}
          <p class="truncate text-sm text-danger" title={d.error} data-part="row-error">{d.error}</p>
        {/if}
        <!-- Narrow screens have no columns: the progress goes under the name. -->
        {#if showBar && !heldForSpace}
          <div class="mt-1.5 grid gap-1 md:hidden">
            <ProgressBar value={barValue} status={d.status} label={t('card.progressOf', { name: d.filename })} />
            {#if brief}<p class="text-xs text-muted tabular-nums">{brief}</p>{/if}
          </div>
        {/if}
      </div>
    </div>
  </td>

  <td class={[CELL, 'whitespace-nowrap max-sm:hidden']}>
    <StatusBadge status={d.status} waiting={heldForSpace} hold={held ? d.hold : null} />
  </td>

  <td class={[CELL, 'min-w-40 max-md:hidden']} data-part="col-progress">
    {#if showBar}
      <div class="grid gap-1">
        <ProgressBar value={barValue} status={d.status} label={t('card.progressOf', { name: d.filename })} />
        <p class="text-xs text-muted tabular-nums">{percent || ' '}</p>
      </div>
    {/if}
  </td>

  <td class={[CELL, 'text-right text-sm whitespace-nowrap tabular-nums max-lg:hidden']} data-part="col-size">
    {d.size != null ? i18n.bytes(d.size) : d.downloaded > 0 ? i18n.bytes(d.downloaded) : '—'}
  </td>

  <td class={[CELL, 'text-right text-sm whitespace-nowrap tabular-nums max-lg:hidden']} data-part="col-speed">
    {live ? i18n.speed(speed) : ''}
  </td>

  <td class={[CELL, 'text-right text-sm whitespace-nowrap tabular-nums max-lg:hidden']} data-part="col-eta">
    {live && eta !== null ? i18n.duration(eta) : ''}
  </td>

  <td class={[CELL, 'w-px whitespace-nowrap']}>
    {#if !selecting}
      <div class="flex items-center justify-end gap-1.5">
        {#if primary}
          <button
            class="btn icon"
            disabled={busy}
            aria-label={primary.label}
            title={primary.label}
            onclick={() => act(primary.run)}
            data-part="primary"
          >
            <Icon name={primary.icon} />
          </button>
        {/if}

        {#if verifyFailed}
          <button
            class="btn icon"
            disabled={busy}
            aria-label={t('card.keepFile')}
            title={t('card.keepFile')}
            onclick={() => act(() => downloads.keep(d.id))}
            data-part="keep"
          >
            <Icon name="check" />
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
            class="btn icon"
            disabled={busy}
            aria-label={t('card.cancel.label', { name: d.filename })}
            title={t('card.cancel.title')}
            onclick={cancel}
          >
            <Icon name="x" />
          </button>
        {:else if verifyFailed}
          <button
            class="btn icon"
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
            class="btn icon"
            disabled={busy}
            aria-label={t('card.remove.label', { name: d.filename })}
            title={d.status === 'completed' || d.status === 'verifying' ? t('card.remove.titleKeepsFile') : t('card.remove.title')}
            onclick={() => act(() => downloads.remove(d.id))}
          >
            <Icon name="trash" />
          </button>
        {/if}
      </div>
    {/if}
  </td>
</tr>
