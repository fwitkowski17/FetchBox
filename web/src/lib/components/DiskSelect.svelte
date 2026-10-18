<script lang="ts">
  // "Save to": a select of the disks with the free space of each, and the space of the chosen one
  // under it. Shown by the dialogs that add something, only when there is a choice.
  import type { Disks } from '../api/types';
  import { i18n, t } from '../i18n';
  import { findDisk } from '../util/disks';

  let {
    id,
    disks,
    value,
    onchange,
  }: {
    id: string;
    disks: Disks;
    /** The chosen disk's name (empty = the download folder). */
    value: string;
    onchange: (name: string) => void;
  } = $props();

  const info = $derived(findDisk(disks, value));
</script>

<div class="field" data-part="disk-select">
  <label for={id}>{t('add.disk')}</label>
  <select {id} class="select" {value} onchange={(e) => onchange(e.currentTarget.value)}>
    {#each disks.disks as d (d.name)}
      <option value={d.name}>
        {t('add.disk.option', { name: d.name === '' ? t('disks.main') : d.name, free: i18n.bytes(d.available) })}
      </option>
    {/each}
  </select>
  {#if info}
    <p class="hint">{t('add.disk.hint', { free: i18n.bytes(info.available), total: i18n.bytes(info.total) })}</p>
  {/if}
</div>
