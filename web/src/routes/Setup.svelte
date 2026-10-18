<script lang="ts">
  // First run, as short as it can be: language, look (theme and accent), password, done. The
  // password step needs the one-time code from the server log. Everything else (download limits,
  // speed unit...) has a sensible default and lives in Settings. One step is added when it is
  // needed: if more than one disk is mounted in the download folder, which one new downloads go
  // to is asked once the account exists (the disks are behind the login, so it cannot come
  // earlier). The same step tells when the download folder itself looks wrongly mounted.
  import { ApiError } from '../lib/api/client';
  import DiskChoice from '../lib/components/DiskChoice.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import LanguageSelect from '../lib/components/LanguageSelect.svelte';
  import LookFields from '../lib/components/LookFields.svelte';
  import { i18n, t } from '../lib/i18n';
  import { auth } from '../lib/stores/auth.svelte';
  import { disks } from '../lib/stores/disks.svelte';
  import { settings } from '../lib/stores/settings.svelte';
  import { warningKey } from '../lib/util/disks';
  import { CARD } from '../lib/util/classes';
  import { MIN_PASSWORD_LEN, checkNewPassword } from '../lib/util/password';

  type Step = 'language' | 'look' | 'account' | 'disk' | 'done';

  /** The disk step is part of the list once the server has told that there is a choice. */
  let askDisk = $state(false);
  /** There is a real choice (otherwise the step only shows a warning). */
  const choose = $derived(disks.choosable);
  const STEPS = $derived<readonly Step[]>(
    askDisk ? ['language', 'look', 'account', 'disk', 'done'] : ['language', 'look', 'account', 'done'],
  );

  let step = $state<Step>('language');
  const index = $derived(STEPS.indexOf(step));

  // ---- disk
  /** `null` until one is chosen: the step has no sensible default (the download folder may be the
   *  small system disk of a container), so the choice is the user's. */
  let chosenDisk = $state<string | null>(null);
  let savingDisk = $state(false);
  let diskError = $state<string | null>(null);

  async function saveDisk(): Promise<void> {
    if (!choose) {
      step = 'done';
      return;
    }
    if (chosenDisk === null || savingDisk) return;
    savingDisk = true;
    diskError = null;
    try {
      if (!settings.value) await settings.load();
      if (settings.value) await settings.save({ ...settings.value, default_disk: chosenDisk });
      step = 'done';
    } catch (err) {
      diskError = err instanceof ApiError ? i18n.errorText(err) : t('setup.disk.failed');
    } finally {
      savingDisk = false;
    }
  }

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
      // Signed in now: see whether there is a disk to choose. Not being able to ask is no reason
      // to hold the user back.
      const found = await disks.load();
      const warned = (found?.disks.some((d) => d.warning) ?? false) || (found?.unmounted?.length ?? 0) > 0;
      askDisk = (found?.choosable ?? false) || warned;
      step = askDisk ? 'disk' : 'done';
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

  function finish(): void {
    location.hash = '#/';
    auth.finishWizard();
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

    {#if step === 'language'}
      <section class={CARD} data-part="step-language">
        <div>
          <h2 class="text-lg font-semibold tracking-tight">{t('setup.language.title')}</h2>
          <p class="hint mt-1">{t('setup.language.hint')}</p>
        </div>
        <LanguageSelect />
        <button class="btn primary justify-self-end" type="button" onclick={() => (step = 'look')}>
          {t('setup.next')}
        </button>
      </section>
    {:else if step === 'look'}
      <section class={CARD} data-part="step-look">
        <div>
          <h2 class="text-lg font-semibold tracking-tight">{t('setup.look.title')}</h2>
          <p class="hint mt-1">{t('setup.look.hint')}</p>
        </div>
        <LookFields />
        <div class="flex justify-between gap-2">
          <button class="btn" type="button" onclick={() => (step = 'language')}>{t('setup.back')}</button>
          <button class="btn primary" type="button" onclick={() => (step = 'account')}>
            {t('setup.next')}
          </button>
        </div>
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
    {:else if step === 'disk'}
      <section class={CARD} data-part="step-disk">
        <div>
          <h2 class="text-lg font-semibold tracking-tight">{t(choose ? 'setup.disk.title' : 'setup.disk.check')}</h2>
          <p class="hint mt-1">{t(choose ? 'setup.disk.hint' : 'setup.disk.checkHint')}</p>
        </div>
        {#if disks.value && choose}
          <DiskChoice disks={disks.value.disks} bind:value={chosenDisk} group="setup-disk" />
        {:else if disks.value}
          {#each disks.value.disks as d (d.name)}
            {#if d.warning}
              <p class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" role="status" data-part="disk-warning">
                <Icon name="alert" size={18} />{t(warningKey(d.warning))}
              </p>
            {/if}
          {/each}
        {/if}
        {#if disks.value && (disks.value.unmounted?.length ?? 0) > 0}
          <p class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" role="status" data-part="disks-unmounted">
            <Icon name="alert" size={18} />{t('disks.unmounted', { names: (disks.value.unmounted ?? []).join(', ') })}
          </p>
        {/if}
        {#if diskError}
          <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
            <Icon name="alert" size={18} />{diskError}
          </p>
        {/if}
        <button
          class="btn primary justify-self-end"
          type="button"
          onclick={saveDisk}
          disabled={(choose && chosenDisk === null) || savingDisk}
          data-part="disk-continue"
        >
          {savingDisk ? t('setup.disk.saving') : t('setup.disk.save')}
        </button>
      </section>
    {:else}
      <section class={[CARD, 'justify-items-center text-center']} data-part="step-done">
        <div class="grid size-12 place-items-center rounded-full bg-success-soft text-success">
          <Icon name="check" size={24} />
        </div>
        <div>
          <h2 class="text-lg font-semibold tracking-tight">{t('setup.done.title')}</h2>
          <p class="hint mt-1">{t('setup.done.hint')}</p>
        </div>
        <button class="btn primary" type="button" onclick={finish} data-part="finish">
          {t('setup.done.open')}
        </button>
      </section>
    {/if}
  </div>
</div>
