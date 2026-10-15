<script lang="ts">
  import { untrack } from 'svelte';
  import { ApiError } from '../lib/api/client';
  import type { Settings as ServerSettings } from '../lib/api/types';
  import Icon from '../lib/components/Icon.svelte';
  import { LOCALES, i18n, t } from '../lib/i18n';
  import SpeedField from '../lib/components/SpeedField.svelte';
  import AccentPicker from '../lib/components/AccentPicker.svelte';
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
    type ErrorCode,
  } from '../lib/util/settingsForm';

  const CARD = 'grid gap-4 rounded-2xl border border-border bg-surface p-5 shadow-card';

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
      toast.success(t('settings.saved'));
    } catch (err) {
      error = err instanceof ApiError ? i18n.errorText(err) : t('settings.saveFailed');
    } finally {
      saving = false;
    }
  }

  /** The message for a validation code; the bounds are the form's own. */
  function errorText(code: ErrorCode): string {
    const max = code === 'connections' ? MAX_CONNECTIONS : MAX_CONCURRENT;
    return t(`settings.error.${code}`, { max });
  }

  function reset(): void {
    if (settings.value) {
      draft = toDraft(settings.value);
      baseline = settings.value;
    }
    error = null;
  }
</script>

<div class="page-narrow">
  <header class="mb-4"><h1 class="text-2xl font-bold tracking-tight">{t('settings.title')}</h1></header>

  <form class="grid gap-4" onsubmit={save}>
    {#if settings.error && !settings.value}
      <div class={CARD} role="alert">
        <p>{t('settings.loadFailed', { error: i18n.errorText(settings.error) })}</p>
        <button type="button" class="btn" onclick={() => settings.load()}>{t('common.tryAgain')}</button>
      </div>
    {:else if draft === null}
      <p class="hint" role="status">{t('common.loading')}</p>
    {:else}
      <section class={CARD}>
        <h2 class="font-semibold tracking-tight">{t('settings.downloads')}</h2>
        <div class="field">
          <label for="connections">{t('settings.connections.label')}</label>
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
          <p class="hint">{t('settings.connections.hint')}</p>
          {#if errors.connections}
            <p class="text-sm text-danger" role="alert">{errorText(errors.connections)}</p>
          {/if}
        </div>

        <div class="field">
          <label for="concurrent">{t('settings.concurrent.label')}</label>
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
          <p class="hint">{t('settings.concurrent.hint')}</p>
          {#if errors.maxConcurrent}
            <p class="text-sm text-danger" role="alert">{errorText(errors.maxConcurrent)}</p>
          {/if}
        </div>
      </section>

      <section class={CARD}>
        <h2 class="font-semibold tracking-tight">{t('settings.speed')}</h2>
        <SpeedField
          label={t('settings.speed.each')}
          hint={t('settings.speed.eachHint')}
          bind:value={draft.perDownload}
          error={errors.perDownload ? errorText(errors.perDownload) : undefined}
        />
        <hr class="m-0 w-full border-0 border-t border-border" />
        <SpeedField
          label={t('settings.speed.total')}
          hint={t('settings.speed.totalHint')}
          bind:value={draft.global}
          error={errors.global ? errorText(errors.global) : undefined}
        />
      </section>

      <section class={CARD}>
        <h2 class="font-semibold tracking-tight">{t('settings.files')}</h2>
        <Toggle bind:checked={draft.categorySubdirs} hint={t('settings.subdirs.hint')}>
          {t('settings.subdirs')}
        </Toggle>
      </section>

      {#if error}
        <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
          <Icon name="alert" size={18} />{error}
        </p>
      {/if}

      {#if dirty}
        <div
          class="sticky bottom-[calc(0.75rem+env(safe-area-inset-bottom))] z-10 flex items-center gap-2 rounded-xl border border-accent bg-surface px-3 py-2 shadow-card"
          role="group"
          aria-label={t('settings.unsaved')}
          data-part="savebar"
        >
          <span class="hint flex-1">{t('settings.unsaved')}</span>
          <button type="button" class="btn" onclick={reset} disabled={saving}>{t('settings.discard')}</button>
          <button type="submit" class="btn primary" disabled={!valid || saving}>
            {saving ? t('settings.saving') : t('settings.save')}
          </button>
        </div>
      {/if}
    {/if}

    <section class={CARD}>
      <h2 class="font-semibold tracking-tight">{t('settings.appearance')}</h2>
      <ThemePicker />
      <p class="hint">{t('settings.theme.hint')}</p>

      <div class="field">
        <span class="label">{t('settings.accent')}</span>
        <AccentPicker />
      </div>

      <div class="field">
        <label for="language">{t('settings.language')}</label>
        <select
          id="language"
          class="select"
          value={i18n.pref}
          onchange={(e) => void i18n.setPref(e.currentTarget.value)}
        >
          <option value="auto">{t('settings.language.auto')}</option>
          {#each LOCALES as l (l.code)}
            <option value={l.code} lang={l.code}>{l.name}</option>
          {/each}
        </select>
      </div>
    </section>

    {#if auth.required}
      <section class={CARD}>
        <h2 class="font-semibold tracking-tight">{t('settings.account')}</h2>
        <button type="button" class="btn" onclick={() => auth.logout()}>
          <Icon name="logout" size={18} />{t('settings.signOut')}
        </button>
      </section>
    {/if}
  </form>
</div>
