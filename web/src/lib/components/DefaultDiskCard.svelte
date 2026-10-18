<script lang="ts">
  // The "Download disk" card of Settings: which disk new downloads are saved on. Only shown when
  // there is something to choose (another disk is mounted in the download directory) or when the
  // disk that was chosen is gone. The choice is part of the settings form: saved with the rest.
  import { onMount } from 'svelte';
  import { t } from '../i18n';
  import { disks } from '../stores/disks.svelte';
  import { CARD } from '../util/classes';
  import { findDisk } from '../util/disks';
  import type { Draft } from '../util/settingsForm';
  import DiskChoice from './DiskChoice.svelte';

  let { draft = $bindable() }: { draft: Draft } = $props();

  // A disk can be mounted while Settings is open elsewhere: ask again on arrival.
  onMount(() => void disks.load());

  const found = $derived(disks.value);
  /** The choice in the form is a disk that is not there (any more). */
  const gone = $derived(
    found !== null && draft.defaultDisk !== '' && findDisk(found, draft.defaultDisk) === undefined
      ? draft.defaultDisk
      : null,
  );
  const show = $derived(found !== null && (found.choosable || gone !== null));
</script>

{#if found && show}
  <section class={CARD} data-part="disks">
    <div>
      <h2 class="font-semibold tracking-tight">{t('settings.disks')}</h2>
      <p class="hint mt-1">{t('settings.disks.hint')}</p>
    </div>
    <DiskChoice
      disks={found.disks}
      bind:value={draft.defaultDisk}
      group="default-disk"
      missing={gone}
    />
    <p class="hint">{t('settings.disks.keep')}</p>
  </section>
{/if}
