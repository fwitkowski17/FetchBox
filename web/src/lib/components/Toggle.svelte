<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    checked: boolean;
    children: Snippet;
    hint?: string;
  }

  let { checked = $bindable(), children, hint }: Props = $props();
</script>

<label class="toggle">
  <span class="text">
    <span class="label">{@render children()}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </span>
  <input type="checkbox" role="switch" bind:checked />
  <span class="track" aria-hidden="true"><span class="thumb"></span></span>
</label>

<style>
  .toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    min-height: var(--tap);
    cursor: pointer;
  }

  .text {
    display: grid;
    gap: 0.1rem;
  }

  .label {
    font-weight: 600;
  }

  input {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
  }

  .track {
    flex: none;
    width: 3rem;
    height: 1.75rem;
    padding: 0.2rem;
    border-radius: 999px;
    background: var(--border);
    transition: background 0.15s;
  }

  .thumb {
    display: block;
    width: 1.35rem;
    height: 1.35rem;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgb(0 0 0 / 35%);
    transition: transform 0.15s;
  }

  input:checked + .track {
    background: var(--accent);
  }

  input:checked + .track .thumb {
    transform: translateX(1.25rem);
  }

  input:focus-visible + .track {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
