<script lang="ts">
  import { flip } from 'svelte/animate';
  import { fade, fly } from 'svelte/transition';
  import { t } from '../i18n';
  import { toast } from '../stores/toast.svelte';
  import { motion } from '../util/motion';
  import Icon from './Icon.svelte';
</script>

<div
  class="pointer-events-none fixed top-[calc(var(--header-h)+env(safe-area-inset-top)+0.5rem)] left-1/2 z-50 grid w-[min(28rem,calc(100vw-1.5rem))] -translate-x-1/2 gap-1.5 lg:top-4 lg:right-4 lg:left-auto lg:w-96 lg:translate-x-0"
  aria-live="polite"
>
  {#each toast.items as item (item.id)}
    <div
      in:fly={{ y: -14, duration: motion(220) }}
      out:fade={{ duration: motion(160) }}
      animate:flip={{ duration: motion(200) }}
      class={[
        'pointer-events-auto flex items-center gap-2 rounded-lg border bg-surface py-1.5 pr-1.5 pl-3 text-sm shadow-card',
        item.kind === 'error' ? 'border-danger' : item.kind === 'warning' ? 'border-warning' : 'border-border',
      ]}
      role={item.kind === 'error' ? 'alert' : 'status'}
    >
      <span
        class={item.kind === 'error' ? 'text-danger' : item.kind === 'warning' ? 'text-warning' : 'text-success'}
      >
        <Icon name={item.kind === 'error' || item.kind === 'warning' ? 'alert' : 'check'} size={18} />
      </span>
      <span class="flex-1 [overflow-wrap:anywhere]">{item.message}</span>
      <button
        class="grid size-8 place-items-center rounded-md text-muted hover:bg-surface-2"
        aria-label={t('common.dismiss')}
        onclick={() => toast.dismiss(item.id)}
      >
        <Icon name="x" size={16} />
      </button>
    </div>
  {/each}
</div>
