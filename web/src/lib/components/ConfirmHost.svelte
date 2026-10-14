<script lang="ts">
  import { confirmStore } from '../stores/confirm.svelte';
  import Modal from './Modal.svelte';

  const pending = $derived(confirmStore.current);
</script>

<Modal open={pending !== null} title={pending?.title ?? ''} onclose={() => confirmStore.answer(false)}>
  <p>{pending?.message}</p>
  {#snippet footer()}
    <button class="btn" onclick={() => confirmStore.answer(false)}>Keep it</button>
    <button
      class="btn"
      class:danger={pending?.danger}
      class:solid={pending?.danger}
      class:primary={!pending?.danger}
      onclick={() => confirmStore.answer(true)}
    >
      {pending?.confirmLabel}
    </button>
  {/snippet}
</Modal>
