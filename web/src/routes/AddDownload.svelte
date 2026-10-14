<script lang="ts">
  import { ApiError, api } from '../lib/api/client';
  import type { Category, Conflict, OnConflict } from '../lib/api/types';
  import { CATEGORIES, CATEGORY_LABELS } from '../lib/api/types';
  import ConflictDialog from '../lib/components/ConflictDialog.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { toast } from '../lib/stores/toast.svelte';
  import { isHttpUrl } from '../lib/util/format';

  let url = $state('');
  let filename = $state('');
  let category = $state<Category | 'auto'>('auto');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let conflict = $state<Conflict | null>(null);

  const urlOk = $derived(isHttpUrl(url));
  const showUrlHint = $derived(url.trim() !== '' && !urlOk);

  async function submit(onConflict?: OnConflict): Promise<void> {
    busy = true;
    error = null;
    try {
      const added = await api.addDownload({
        url: url.trim(),
        filename: filename.trim() || undefined,
        category: category === 'auto' ? undefined : category,
        on_conflict: onConflict,
      });
      conflict = null;
      toast.success(`Added “${added.filename}”`);
      url = '';
      filename = '';
      category = 'auto';
      location.hash = '#/';
    } catch (e) {
      if (e instanceof ApiError && e.status === 409 && e.conflict) {
        conflict = e.conflict; // the dialog lets the user choose, then we send again
      } else {
        conflict = null;
        error = e instanceof Error ? e.message : 'Could not add the download.';
      }
    } finally {
      busy = false;
    }
  }

  function onSubmit(e: SubmitEvent): void {
    e.preventDefault();
    if (urlOk && !busy) void submit();
  }

  function choose(choice: OnConflict | null): void {
    if (choice === null) {
      conflict = null; // backing out simply does not send the request again
    } else {
      void submit(choice);
    }
  }
</script>

<header class="head"><h1>Add download</h1></header>

<form class="card" onsubmit={onSubmit}>
  <div class="field">
    <label for="url">Address</label>
    <!-- svelte-ignore a11y_autofocus -->
    <input
      id="url"
      class="input"
      type="url"
      inputmode="url"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      autofocus
      placeholder="https://example.com/file.zip"
      bind:value={url}
      aria-invalid={showUrlHint ? 'true' : undefined}
      aria-describedby="url-hint"
    />
    <p id="url-hint" class="hint" class:warn={showUrlHint}>
      {showUrlHint ? 'Enter a full http:// or https:// address.' : 'Direct link to a file.'}
    </p>
  </div>

  <div class="two">
    <div class="field">
      <label for="filename">File name <span class="opt">(optional)</span></label>
      <input id="filename" class="input" autocomplete="off" placeholder="Taken from the server" bind:value={filename} />
    </div>
    <div class="field">
      <label for="category">Category</label>
      <select id="category" class="select" bind:value={category}>
        <option value="auto">Detect automatically</option>
        {#each CATEGORIES as c (c)}
          <option value={c}>{CATEGORY_LABELS[c]}</option>
        {/each}
      </select>
    </div>
  </div>

  {#if error}
    <p class="error" role="alert"><Icon name="alert" size={18} />{error}</p>
  {/if}

  <button class="btn primary" type="submit" disabled={!urlOk || busy}>
    <Icon name="down" size={18} />{busy && !conflict ? 'Adding…' : 'Start download'}
  </button>
</form>

<ConflictDialog {conflict} {busy} onchoose={choose} />

<style>
  .head {
    margin-bottom: 1rem;
  }

  .card {
    display: grid;
    gap: 1rem;
    padding: 1rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  .two {
    display: grid;
    gap: 1rem;
  }

  @media (min-width: 40rem) {
    .two {
      grid-template-columns: 1fr 1fr;
    }
  }

  .opt {
    color: var(--muted);
    font-weight: 400;
  }

  .warn {
    color: var(--warning);
  }

  .error {
    display: flex;
    gap: 0.5rem;
    align-items: flex-start;
    padding: 0.6rem 0.75rem;
    border-radius: var(--radius-sm);
    background: var(--danger-soft);
    color: var(--danger);
    overflow-wrap: anywhere;
  }
</style>
