<script lang="ts">
  // First run: look, account, download defaults, done. The account step needs the one-time code
  // from the server log; the steps after it already run signed in.
  import { untrack } from 'svelte';
  import { ApiError } from '../lib/api/client';
  import AppearanceFields from '../lib/components/AppearanceFields.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import SettingsFields from '../lib/components/SettingsFields.svelte';
  import { i18n, t } from '../lib/i18n';
  import { addDialog } from '../lib/stores/addDialog.svelte';
  import { auth } from '../lib/stores/auth.svelte';
  import { settings } from '../lib/stores/settings.svelte';
  import { CARD } from '../lib/util/classes';
  import { MIN_PASSWORD_LEN, checkNewPassword } from '../lib/util/password';
  import { fromDraft, toDraft, validate, type Draft } from '../lib/util/settingsForm';

  const STEPS = ['look', 'account', 'downloads', 'done'] as const;
  type Step = (typeof STEPS)[number];

  let step = $state<Step>('look');
  const index = $derived(STEPS.indexOf(step));

  // ---- account
  let code = $state('');
  let password = $state('');
  let repeat = $state('');
  let creating = $state(false);
  let accountError = $state<string | null>(null);
  let codeWrong = $state(false);
  let triedAccount = $state(false);

  const problem = $derived(checkNewPassword(password, repeat));
  const problemText = $derived(
    problem === 'mismatch' ? t('setup.error.mismatch') : t('setup.error.short', { min: MIN_PASSWORD_LEN }),
  );

  async function createAccount(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    triedAccount = true;
    if (creating || !code.trim() || problem) return;
    creating = true;
    accountError = null;
    codeWrong = false;
    try {
      await auth.setup(code, password);
      password = repeat = code = '';
      step = 'downloads';
    } catch (err) {
      if (err instanceof ApiError && err.status === 403) {
        accountError = t('setup.error.code');
        codeWrong = true;
      }
      else if (err instanceof ApiError && err.status === 409) accountError = t('setup.error.done');
      else if (err instanceof ApiError && err.status === 429) {
        accountError = t('login.locked', { n: err.retryAfter ?? 60 });
      } else if (err instanceof ApiError && err.status === 400) {
        accountError = t('setup.error.short', { min: MIN_PASSWORD_LEN });
      } else if (err instanceof ApiError && err.status === 0) {
        void auth.check(); // the server is gone: the connection dialog takes over
        accountError = i18n.errorText(err);
      } else {
        accountError = t('setup.error.failed');
      }
    } finally {
      creating = false;
    }
  }

  // ---- download defaults
  let draft = $state<Draft | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);

  // The settings load as soon as the account step signs us in.
  $effect(() => {
    const current = settings.value;
    if (current) untrack(() => (draft ??= toDraft(current)));
  });

  const errors = $derived(draft ? validate(draft) : {});
  const valid = $derived(Object.keys(errors).length === 0);

  async function saveDefaults(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (!draft || !valid || saving) return;
    saving = true;
    saveError = null;
    try {
      await settings.save(fromDraft(draft));
      step = 'done';
    } catch (err) {
      saveError = err instanceof ApiError ? i18n.errorText(err) : t('settings.saveFailed');
    } finally {
      saving = false;
    }
  }

  function finish(thenAdd: boolean): void {
    location.hash = '#/';
    auth.finishWizard();
    if (thenAdd) addDialog.show();
  }
</script>

<div class="grid min-h-dvh content-start justify-items-center gap-6 p-4 pt-[max(1.5rem,env(safe-area-inset-top))] sm:pt-12" data-part="setup">
  <header class="grid justify-items-center gap-2 text-center">
    <div class="grid size-14 place-items-center rounded-2xl bg-accent text-accent-contrast">
      <Icon name="down" size={26} />
    </div>
    <h1 class="text-2xl font-bold tracking-tight">{t('setup.title')}</h1>
  </header>

  <div class="grid w-[min(32rem,100%)] gap-4">
    <nav aria-label={t('setup.steps')}>
      <ol class="flex items-center gap-2 text-sm">
        {#each STEPS as s, i (s)}
          <li
            class={[
              'flex flex-1 flex-col gap-1.5',
              i === index ? 'font-semibold text-fg' : 'text-muted',
            ]}
            aria-current={i === index ? 'step' : undefined}
            data-step={s}
            data-state={i < index ? 'done' : i === index ? 'current' : 'todo'}
          >
            <span class={['h-1 rounded-full', i <= index ? 'bg-accent' : 'bg-border']}></span>
            <span class="truncate">{t(`setup.step.${s}`)}</span>
          </li>
        {/each}
      </ol>
      <p class="sr-only" role="status">{t('setup.step', { current: index + 1, total: STEPS.length })}</p>
    </nav>

    {#if step === 'look'}
      <section class={CARD} data-part="step-look">
        <div>
          <h2 class="text-lg font-semibold tracking-tight">{t('setup.look.title')}</h2>
          <p class="hint mt-1">{t('setup.look.hint')}</p>
        </div>
        <AppearanceFields />
        <button class="btn primary justify-self-end" type="button" onclick={() => (step = 'account')}>
          {t('setup.next')}
        </button>
      </section>
    {:else if step === 'account'}
      <form class={CARD} onsubmit={createAccount} data-part="step-account">
        <div>
          <h2 class="text-lg font-semibold tracking-tight">{t('setup.account.title')}</h2>
          <p class="hint mt-1">{t('setup.account.hint')}</p>
        </div>

        <div class="field">
          <label for="setup-code">{t('setup.code.label')}</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            id="setup-code"
            class="input font-mono tracking-widest uppercase"
            type="text"
            autocomplete="off"
            autocapitalize="characters"
            spellcheck="false"
            autofocus
            bind:value={code}
            aria-invalid={codeWrong ? 'true' : undefined}
            aria-describedby="setup-code-hint"
          />
          <p id="setup-code-hint" class="hint">{t('setup.code.hint')}</p>
        </div>

        <div class="field">
          <label for="setup-password">{t('setup.password.label')}</label>
          <input
            id="setup-password"
            class="input"
            type="password"
            autocomplete="new-password"
            bind:value={password}
            aria-invalid={triedAccount && problem && problem !== 'mismatch' ? 'true' : undefined}
          />
          <p class="hint">{t('setup.password.hint', { min: MIN_PASSWORD_LEN })}</p>
        </div>

        <div class="field">
          <label for="setup-repeat">{t('setup.confirm.label')}</label>
          <input
            id="setup-repeat"
            class="input"
            type="password"
            autocomplete="new-password"
            bind:value={repeat}
            aria-invalid={triedAccount && problem === 'mismatch' ? 'true' : undefined}
          />
        </div>

        {#if triedAccount && problem}
          <p class="text-sm text-danger" role="alert">{problemText}</p>
        {/if}
        {#if accountError}
          <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
            <Icon name="alert" size={18} />{accountError}
          </p>
        {/if}

        <div class="flex justify-between gap-2">
          <button class="btn" type="button" onclick={() => (step = 'look')} disabled={creating}>
            {t('setup.back')}
          </button>
          <button class="btn primary" type="submit" disabled={creating || !code.trim() || !password}>
            {creating ? t('setup.creating') : t('setup.create')}
          </button>
        </div>
      </form>
    {:else if step === 'downloads'}
      <form class="grid gap-4" onsubmit={saveDefaults} data-part="step-downloads">
        <div>
          <h2 class="text-lg font-semibold tracking-tight">{t('setup.downloads.title')}</h2>
          <p class="hint mt-1">{t('setup.downloads.hint')}</p>
        </div>

        {#if draft}
          <SettingsFields bind:draft {errors} />
        {:else if settings.error}
          <p class="text-danger" role="alert">
            {t('settings.loadFailed', { error: i18n.errorText(settings.error) })}
          </p>
        {:else}
          <p class="hint" role="status">{t('common.loading')}</p>
        {/if}

        {#if saveError}
          <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
            <Icon name="alert" size={18} />{saveError}
          </p>
        {/if}

        <div class="flex justify-between gap-2">
          <button class="btn" type="button" onclick={() => (step = 'done')} disabled={saving}>
            {t('setup.skip')}
          </button>
          <button class="btn primary" type="submit" disabled={!draft || !valid || saving}>
            {saving ? t('setup.saving') : t('setup.saveContinue')}
          </button>
        </div>
      </form>
    {:else}
      <section class={[CARD, 'justify-items-center text-center']} data-part="step-done">
        <div class="grid size-12 place-items-center rounded-full bg-success-soft text-success">
          <Icon name="check" size={24} />
        </div>
        <div>
          <h2 class="text-lg font-semibold tracking-tight">{t('setup.done.title')}</h2>
          <p class="hint mt-1">{t('setup.done.hint')}</p>
        </div>
        <div class="flex flex-wrap justify-center gap-2">
          <button class="btn" type="button" onclick={() => finish(false)}>{t('setup.done.list')}</button>
          <button class="btn primary" type="button" onclick={() => finish(true)}>
            {t('setup.done.add')}
          </button>
        </div>
      </section>
    {/if}
  </div>
</div>
