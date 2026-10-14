<script lang="ts">
  import { untrack } from 'svelte';
  import { ApiError } from '../lib/api/client';
  import type { Settings as ServerSettings } from '../lib/api/types';
  import Icon from '../lib/components/Icon.svelte';
  import SpeedField from '../lib/components/SpeedField.svelte';
  import ThemePicker from '../lib/components/ThemePicker.svelte';
  import Toggle from '../lib/components/Toggle.svelte';
  import { auth } from '../lib/stores/auth.svelte';
  import { settings } from '../lib/stores/settings.svelte';
  import { toast } from '../lib/stores/toast.svelte';
  import {
    MAX_CONCURRENT,
    MAX_CONNECTIONS,
    fromDraft,
    isUntouched,
    sameSettings,
    toDraft,
    validate,
    type Draft,
  } from '../lib/util/settingsForm';

  let draft = $state<Draft | null>(null);
  /** The server value the form was last filled from. */
  let baseline = $state.raw<ServerSettings | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);

  const dirty = $derived(
    draft !== null && settings.value !== null && !sameSettings(fromDraft(draft), settings.value),
  );
  const errors = $derived(draft ? validate(draft) : {});
  const valid = $derived(Object.keys(errors).length === 0);

  // Follow the server (first load, or another client saved) unless there are unsaved edits.
  $effect(() => {
    const current = settings.value;
    if (!current) return;
    untrack(() => {
      if (isUntouched(draft, baseline)) draft = toDraft(current);
      baseline = current;
    });
  });

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (!draft || !valid || saving) return;
    saving = true;
    error = null;
    try {
      await settings.save(fromDraft(draft));
      toast.success('Settings saved. They apply right away.');
    } catch (err) {
      error = err instanceof ApiError ? err.message : 'Could not save the settings.';
    } finally {
      saving = false;
    }
  }

  function reset(): void {
    if (settings.value) {
      draft = toDraft(settings.value);
      baseline = settings.value;
    }
    error = null;
  }
</script>

<header class="head"><h1>Settings</h1></header>

<form class="stack" onsubmit={save}>
  {#if settings.error && !settings.value}
    <div class="card" role="alert">
      <p>Could not load the settings: {settings.error}</p>
      <button type="button" class="btn" onclick={() => settings.load()}>Try again</button>
    </div>
  {:else if draft === null}
    <p class="hint" role="status">Loading…</p>
  {:else}
    <section class="card">
      <h2>Downloads</h2>
      <div class="field">
        <label for="connections">Connections per download</label>
        <input
          id="connections"
          class="input"
          type="number"
          inputmode="numeric"
          min="1"
          max={MAX_CONNECTIONS}
          bind:value={draft.connections}
          aria-invalid={errors.connections ? 'true' : undefined}
        />
        <p class="hint">More connections can be faster, but only for servers that allow it. Applies to new downloads.</p>
        {#if errors.connections}<p class="error" role="alert">{errors.connections}</p>{/if}
      </div>

      <div class="field">
        <label for="concurrent">Simultaneous downloads</label>
        <input
          id="concurrent"
          class="input"
          type="number"
          inputmode="numeric"
          min="1"
          max={MAX_CONCURRENT}
          bind:value={draft.maxConcurrent}
          aria-invalid={errors.maxConcurrent ? 'true' : undefined}
        />
        <p class="hint">Extra downloads wait in the queue. Changes take effect immediately.</p>
        {#if errors.maxConcurrent}<p class="error" role="alert">{errors.maxConcurrent}</p>{/if}
      </div>
    </section>

    <section class="card">
      <h2>Speed limits</h2>
      <SpeedField
        label="Limit each download"
        hint="Applies to every download separately."
        bind:value={draft.perDownload}
        error={errors.perDownload}
      />
      <hr />
      <SpeedField
        label="Limit total speed"
        hint="Shared by all downloads together."
        bind:value={draft.global}
        error={errors.global}
      />
    </section>

    <section class="card">
      <h2>Files</h2>
      <Toggle
        bind:checked={draft.categorySubdirs}
        hint="New downloads go into folders like video/ or music/. Existing files are not moved."
      >
        Sort into category folders
      </Toggle>
    </section>

    {#if error}
      <p class="banner" role="alert"><Icon name="alert" size={18} />{error}</p>
    {/if}

    {#if dirty}
      <div class="savebar" role="group" aria-label="Unsaved changes">
        <span class="hint">Unsaved changes</span>
        <button type="button" class="btn" onclick={reset} disabled={saving}>Discard</button>
        <button type="submit" class="btn primary" disabled={!valid || saving}>
          {saving ? 'Saving…' : 'Save changes'}
        </button>
      </div>
    {/if}
  {/if}

  <section class="card">
    <h2>Appearance</h2>
    <ThemePicker />
    <p class="hint">“System” follows the light or dark setting of your device.</p>
  </section>

  {#if auth.required}
    <section class="card">
      <h2>Account</h2>
      <button type="button" class="btn" onclick={() => auth.logout()}>
        <Icon name="logout" size={18} />Sign out
      </button>
    </section>
  {/if}
</form>

<style>
  .head {
    margin-bottom: 1rem;
  }

  .stack {
    display: grid;
    gap: 1rem;
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

  h2 {
    font-size: 1.05rem;
  }

  hr {
    width: 100%;
    margin: 0;
    border: 0;
    border-top: 1px solid var(--border);
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .banner {
    display: flex;
    gap: 0.5rem;
    padding: 0.6rem 0.75rem;
    border-radius: var(--radius-sm);
    background: var(--danger-soft);
    color: var(--danger);
  }

  .savebar {
    position: sticky;
    bottom: calc(var(--nav-h) + 0.5rem + env(safe-area-inset-bottom));
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.6rem 0.75rem;
    background: var(--surface);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  .savebar .hint {
    flex: 1;
  }

  @media (min-width: 48rem) {
    .savebar {
      bottom: 1rem;
    }
  }
</style>
