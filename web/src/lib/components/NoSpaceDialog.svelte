<script lang="ts">
  // A new download does not fit next to what is already reserved. The user decides: the numbers
  // are shown so the choice is informed, and "add anyway" is a real option (files may be about to
  // be moved or deleted).
  import type { SpaceInfo } from '../api/client';
  import { i18n, t } from '../i18n';
  import Modal from './Modal.svelte';
  import Trans from './Trans.svelte';

  interface Props {
    space: SpaceInfo | null;
    busy: boolean;
    /** `true`: add it anyway. `false`: the user backed out. */
    onchoose: (addAnyway: boolean) => void;
  }

  let { space, busy, onchoose }: Props = $props();

  const left = $derived(space ? Math.max(0, space.free - space.reserved) : 0);
</script>

<Modal open={space !== null} title={t('nospace.title')} onclose={() => !busy && onchoose(false)}>
  {#if space}
    <p data-part="nospace-text">
      {#if space.reserved > 0}
        <Trans
          message="nospace.reserved"
          params={{
            needed: i18n.bytes(space.needed),
            free: i18n.bytes(space.free),
            reserved: i18n.bytes(space.reserved),
            left: i18n.bytes(left),
          }}
        />
      {:else}
        <Trans
          message="nospace.plain"
          params={{ needed: i18n.bytes(space.needed), free: i18n.bytes(space.free) }}
        />
      {/if}
    </p>
    <p class="hint">{t('nospace.hint')}</p>
  {/if}

  {#snippet footer()}
    <button class="btn" disabled={busy} onclick={() => onchoose(false)}>{t('common.cancel')}</button>
    <button class="btn primary" disabled={busy} onclick={() => onchoose(true)}>
      {t('nospace.add')}
    </button>
  {/snippet}
</Modal>
