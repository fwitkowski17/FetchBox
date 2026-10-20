<script lang="ts">
  // A row of tabs: roving tabindex, Left/Right/Home/End. The panel is the caller's; give it
  // `role="tabpanel"` and `aria-labelledby="{idPrefix}-tab-{selected}"`.
  import Icon, { type IconName } from './Icon.svelte';

  interface Tab {
    id: string;
    label: string;
    icon?: IconName;
    /** A small dot: something in this tab wants a look. */
    dot?: boolean;
  }

  interface Props {
    tabs: readonly Tab[];
    selected: string;
    onselect: (id: string) => void;
    /** Accessible name of the row. */
    label: string;
    idPrefix: string;
  }

  let { tabs, selected, onselect, label, idPrefix }: Props = $props();

  function onKeydown(e: KeyboardEvent): void {
    const i = Math.max(0, tabs.findIndex((tab) => tab.id === selected));
    let next = i;
    if (e.key === 'ArrowRight') next = (i + 1) % tabs.length;
    else if (e.key === 'ArrowLeft') next = (i + tabs.length - 1) % tabs.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = tabs.length - 1;
    else return;
    e.preventDefault();
    const target = tabs[next];
    if (!target) return;
    onselect(target.id);
    document.getElementById(`${idPrefix}-tab-${target.id}`)?.focus();
  }
</script>

<div
  class="flex gap-1 overflow-x-auto border-b border-border"
  role="tablist"
  aria-label={label}
  tabindex="-1"
  onkeydown={onKeydown}
>
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      role="tab"
      id="{idPrefix}-tab-{tab.id}"
      aria-selected={selected === tab.id}
      aria-controls="{idPrefix}-tabpanel"
      tabindex={selected === tab.id ? 0 : -1}
      class={[
        '-mb-px inline-flex items-center gap-2 whitespace-nowrap border-b-2 px-3 py-2.5 text-sm font-medium transition-colors',
        selected === tab.id ? 'border-accent text-accent' : 'border-transparent text-muted hover:text-text',
      ]}
      onclick={() => onselect(tab.id)}
      data-tab={tab.id}
    >
      {#if tab.icon}<Icon name={tab.icon} size={18} />{/if}
      {tab.label}
      {#if tab.dot}<span class="size-2 rounded-full bg-warning" aria-hidden="true"></span>{/if}
    </button>
  {/each}
</div>
