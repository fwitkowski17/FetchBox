<script lang="ts">
  import Downloads from './routes/Downloads.svelte';
  import AddDownload from './routes/AddDownload.svelte';
  import Settings from './routes/Settings.svelte';
  import { route, routes } from './lib/stores/route';
</script>

<main>
  {#if $route === 'downloads'}
    <Downloads />
  {:else if $route === 'add'}
    <AddDownload />
  {:else}
    <Settings />
  {/if}
</main>

<nav aria-label="Main">
  {#each routes as r (r.id)}
    <a href={'#' + r.path} class:active={$route === r.id} aria-current={$route === r.id ? 'page' : undefined}>
      {r.label}
    </a>
  {/each}
</nav>

<style>
  main {
    max-width: 48rem;
    margin: 0 auto;
    padding: 1rem 1rem calc(var(--nav-h) + 1.5rem + env(safe-area-inset-bottom));
  }

  nav {
    position: fixed;
    inset: auto 0 0 0;
    display: flex;
    height: calc(var(--nav-h) + env(safe-area-inset-bottom));
    padding-bottom: env(safe-area-inset-bottom);
    background: var(--surface);
    border-top: 1px solid #374151;
  }

  a {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    text-decoration: none;
    font-weight: 500;
  }

  a.active {
    color: var(--accent);
  }
</style>
