<script lang="ts">
  // The "Download disk" card of Settings: which disk new downloads (and, separately if wanted,
  // new torrents) are saved on, and what looks wrong with the way the disks are mounted. Shown
  // when there is something to choose (another disk is mounted in the download folder), when the
  // chosen disk is gone, or when a disk looks like it was not mounted as intended. The choices are
  // part of the settings form: saved with the rest.
  import { onMount } from 'svelte';
  import { t } from '../i18n';
  import { disks } from '../stores/disks.svelte';
  import { CARD } from '../util/classes';
  import { findDisk, warningKey } from '../util/disks';
  import type { Draft } from '../util/settingsForm';
  import DiskChoice from './DiskChoice.svelte';
  import Icon from './Icon.svelte';

  let { draft = $bindable() }: { draft: Draft } = $props();

  // A disk can be mounted while Settings is open elsewhere: ask again on arrival.
  onMount(() => void disks.load());

  /** The value of the "same as downloads" option of the torrent select (no disk has this name). */
  const SAME = '\u0000same';

  const found = $derived(disks.value);
  /** A choice in the form is a disk that is not there (any more). */
  function gone(name: string | null): string | null {
    return found !== null && name !== null && name !== '' && findDisk(found, name) === undefined ? name : null;
  }
  const goneDownloads = $derived(gone(draft.defaultDisk));
  const goneTorrents = $derived(gone(draft.torrentDisk));
  const unmounted = $derived(found?.unmounted ?? []);
  const warned = $derived(found !== null && (unmounted.length > 0 || found.disks.some((d) => d.warning)));
  const show = $derived(
    found !== null && (found.choosable || goneDownloads !== null || goneTorrents !== null || warned),
  );

  function pickTorrentDisk(value: string): void {
    draft.torrentDisk = value === SAME ? null : value;
  }
</script>

{#if found && show}
  <section class={CARD} data-part="disks">
    <div>
      <h2 class="font-semibold tracking-tight">{t('settings.disks')}</h2>
      {#if found.choosable}<p class="hint mt-1">{t('settings.disks.hint')}</p>{/if}
    </div>

    {#if found.choosable}
      <DiskChoice
        disks={found.disks}
        bind:value={draft.defaultDisk}
        group="default-disk"
        missing={goneDownloads}
      />
    {:else}
      <!-- Nothing to choose, but the download folder itself may be doubtful. -->
      {#each found.disks as d (d.name)}
        {#if d.warning}
          <p class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" role="status" data-part="disk-warning" data-warning={d.warning}>
            <Icon name="alert" size={18} />{t(warningKey(d.warning))}
          </p>
        {/if}
      {/each}
    {/if}

    {#if unmounted.length > 0}
      <p class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" role="status" data-part="disks-unmounted">
        <Icon name="alert" size={18} />{t('disks.unmounted', { names: unmounted.join(', ') })}
      </p>
    {/if}

    {#if found.choosable}
      <div class="field" data-part="torrent-disk">
        <label for="torrent-disk">{t('settings.disks.torrents')}</label>
        <select
          id="torrent-disk"
          class="select"
          value={draft.torrentDisk ?? SAME}
          onchange={(e) => pickTorrentDisk(e.currentTarget.value)}
        >
          <option value={SAME}>{t('settings.disks.torrents.same')}</option>
          {#each found.disks as d (d.name)}
            <option value={d.name}>{d.name === '' ? t('disks.main') : d.name}</option>
          {/each}
          {#if goneTorrents !== null}<option value={goneTorrents}>{goneTorrents}</option>{/if}
        </select>
        {#if goneTorrents !== null}
          <p class="text-sm text-warning" role="status">{t('disks.missing', { name: goneTorrents })}</p>
        {:else}
          <p class="hint">{t('settings.disks.torrents.hint')}</p>
        {/if}
      </div>
      <p class="hint">{t('settings.disks.keep')}</p>
    {/if}
  </section>
{/if}
