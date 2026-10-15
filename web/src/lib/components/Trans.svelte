<script lang="ts">
  import { i18n, type MessageKey } from '../i18n';
  import type { Params } from '../i18n/types';

  interface Props {
    message: MessageKey;
    params?: Params;
  }

  let { message, params }: Props = $props();
</script>

<!-- Substituted values (file names, sizes) are bold. Text only: nothing here is parsed as HTML. -->
{#each i18n.parts(message, params) as part, i (i)}{#if part.value}<strong class="v">{part.text}</strong>{:else}{part.text}{/if}{/each}

<style>
  .v {
    overflow-wrap: anywhere;
  }
</style>
