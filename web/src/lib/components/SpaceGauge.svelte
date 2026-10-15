<script lang="ts">
  // For a download held back because the disk is full: how much it needs against how much is
  // free, drawn as one bar (the whole bar is the need; the filled part is what the free space
  // covers, the striped rest is what is missing). Follows the disk figures, so the bar fills up
  // while the user frees space.
  import { i18n, t } from '../i18n';
  import { disk } from '../stores/disk.svelte';
  import { spaceNeed } from '../util/space';

  interface Props {
    size: number;
    downloaded: number;
    /** Say what to do while there is not enough room (the card does; a dialog has its own text). */
    hint?: boolean;
  }

  let { size, downloaded, hint = true }: Props = $props();

  const need = $derived(spaceNeed(size, downloaded, disk.value?.available ?? null));
  const percent = $derived(Math.round(need.covered * 1000) / 10);
</script>

<div
  class="grid gap-2 rounded-lg border border-warning bg-warning-soft px-3 py-2.5 text-sm"
  data-part="space-gauge"
  data-state={need.room ? 'room' : 'short'}
>
  {#if need.room}
    <p class="text-warning">{t('space.room')}</p>
  {:else if hint}
    <p class="text-warning">{t('space.hint')}</p>
  {/if}

  <div
    class="relative h-3 overflow-hidden rounded-full bg-surface"
    role="img"
    aria-label={t('space.aria', {
      free: i18n.bytes(need.free ?? 0),
      needed: i18n.bytes(need.needed),
    })}
  >
    <!-- The missing part: stripes -->
    <div
      class="absolute inset-0 bg-[repeating-linear-gradient(135deg,var(--danger)_0_5px,transparent_5px_10px)] opacity-40"
      hidden={need.room}
    ></div>
    <div
      class={['absolute inset-y-0 left-0 rounded-full', need.room ? 'bg-success' : 'bg-accent']}
      style:width="{percent}%"
      data-part="space-fill"
    ></div>
  </div>

  <dl class="grid grid-cols-3 gap-2 tabular-nums">
    <div>
      <dt class="text-xs text-muted">{t('space.free')}</dt>
      <dd class="font-semibold">{need.free === null ? '—' : i18n.bytes(need.free)}</dd>
    </div>
    <div>
      <dt class="text-xs text-muted">{t('space.needs')}</dt>
      <dd class="font-semibold">{i18n.bytes(need.needed)}</dd>
    </div>
    <div>
      <dt class="text-xs text-muted">{t('space.missing')}</dt>
      <dd class={['font-semibold', need.missing > 0 ? 'text-danger' : 'text-success']}>
        {i18n.bytes(need.missing)}
      </dd>
    </div>
  </dl>
</div>
