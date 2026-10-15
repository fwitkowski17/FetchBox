<script lang="ts">
  import { untrack } from 'svelte';
  import { fly } from 'svelte/transition';
  import { ApiError } from '../lib/api/client';
  import type { Settings as ServerSettings } from '../lib/api/types';
  import Icon from '../lib/components/Icon.svelte';
  import { i18n, t } from '../lib/i18n';
  import SettingsFields from '../lib/components/SettingsFields.svelte';
  import AppearanceFields from '../lib/components/AppearanceFields.svelte';
  import PasswordForm from '../lib/components/PasswordForm.svelte';
  import { auth } from '../lib/stores/auth.svelte';
  import { settings } from '../lib/stores/settings.svelte';
  import { toast } from '../lib/stores/toast.svelte';
  import { CARD } from '../lib/util/classes';
  import { motion } from '../lib/util/motion';
  import {
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
      toast.success(t('settings.saved'));
    } catch (err) {
      error = err instanceof ApiError ? i18n.errorText(err) : t('settings.saveFailed');
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

<div class={["page-narrow", dirty && "pb-28"]}>
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
      <SettingsFields bind:draft {errors} />

    {/if}

    <section class={CARD}>
      <h2 class="font-semibold tracking-tight">{t('settings.appearance')}</h2>
      <AppearanceFields />
    </section>

    <!-- Fixed to the bottom while there is something to save, whatever the scroll position. -->
    {#if draft !== null && dirty}
      <div
        class="fixed inset-x-0 bottom-0 z-20 border-t border-border bg-surface px-3 pt-2 pb-[calc(0.5rem+env(safe-area-inset-bottom))] shadow-[0_-6px_20px_rgb(0_0_0/0.08)] lg:left-[var(--sidebar-w)]"
        role="group"
        aria-label={t('settings.unsaved')}
        data-part="savebar"
        transition:fly={{ y: 40, duration: motion(180) }}
      >
        <div class="mx-auto max-w-[76rem] lg:px-5">
          <div class="page-narrow grid gap-2">
            {#if error}
              <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
                <Icon name="alert" size={18} />{error}
              </p>
            {/if}
            <div class="flex items-center gap-2">
              <span class="hint flex-1">{t('settings.unsaved')}</span>
              <button type="button" class="btn" onclick={reset} disabled={saving}>{t('settings.discard')}</button>
              <button type="submit" class="btn primary" disabled={!valid || saving}>
                {saving ? t('settings.saving') : t('settings.save')}
              </button>
            </div>
          </div>
        </div>
      </div>
    {/if}
  </form>

  {#if auth.required}
    <div class="mt-4 grid gap-4">
      <section class={CARD}>
        <h2 class="font-semibold tracking-tight">{t('settings.account')}</h2>
        <button type="button" class="btn" onclick={() => auth.logout()}>
          <Icon name="logout" size={18} />{t('settings.signOut')}
        </button>
      </section>
      <PasswordForm />
    </div>
  {/if}
</div>
