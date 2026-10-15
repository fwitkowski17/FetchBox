<script lang="ts">
  import { t } from '../i18n';
  import { toast } from '../stores/toast.svelte';
  import Icon from './Icon.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each toast.items as item (item.id)}
    <div class="toast {item.kind}" role={item.kind === 'error' ? 'alert' : 'status'}>
      <Icon name={item.kind === 'error' ? 'alert' : 'check'} size={18} />
      <span>{item.message}</span>
      <button class="close" aria-label={t('common.dismiss')} onclick={() => toast.dismiss(item.id)}>
        <Icon name="x" size={16} />
      </button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    z-index: 50;
    left: 50%;
    top: calc(var(--header-h) + env(safe-area-inset-top) + 0.5rem);
    display: grid;
    gap: 0.35rem;
    width: min(28rem, calc(100vw - 1.5rem));
    transform: translateX(-50%);
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.4rem 0.4rem 0.7rem;
    font-size: 0.9rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    box-shadow: var(--shadow);
    pointer-events: auto;
  }

  .toast span {
    flex: 1;
    overflow-wrap: anywhere;
  }

  .toast.success :global(svg:first-child) {
    color: var(--success);
  }

  .toast.error {
    border-color: var(--danger);
  }

  .toast.error :global(svg:first-child) {
    color: var(--danger);
  }

  .close {
    display: grid;
    place-items: center;
    width: 1.9rem;
    height: 1.9rem;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }

  @media (min-width: 64rem) {
    .toasts {
      top: 1rem;
      left: auto;
      right: 1rem;
      width: 24rem;
      transform: none;
    }
  }
</style>
