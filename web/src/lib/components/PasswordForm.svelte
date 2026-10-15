<script lang="ts">
  // Change the password of an account created in the UI. Hidden text only: `autocomplete`
  // hints let password managers update the saved entry.
  import { ApiError } from '../api/client';
  import { i18n, t } from '../i18n';
  import { auth } from '../stores/auth.svelte';
  import { toast } from '../stores/toast.svelte';
  import { CARD } from '../util/classes';
  import { MIN_PASSWORD_LEN, checkNewPassword } from '../util/password';
  import Icon from './Icon.svelte';

  let current = $state('');
  let next = $state('');
  let repeat = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** Show the validation message only after the user tried to submit. */
  let submitted = $state(false);

  const problem = $derived(checkNewPassword(next, repeat));
  const problemText = $derived(
    problem === 'mismatch' ? t('setup.error.mismatch') : t('setup.error.short', { min: MIN_PASSWORD_LEN }),
  );

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    submitted = true;
    if (busy || !current || problem) return;
    busy = true;
    error = null;
    try {
      await auth.changePassword(current, next);
      current = next = repeat = '';
      submitted = false;
      toast.success(t('settings.password.changed'));
    } catch (err) {
      if (err instanceof ApiError && err.status === 403) error = t('settings.password.wrong');
      else if (err instanceof ApiError && err.status === 429) {
        error = t('login.locked', { n: err.retryAfter ?? 60 });
      } else error = err instanceof ApiError ? i18n.errorText(err) : t('settings.password.failed');
    } finally {
      busy = false;
    }
  }
</script>

<section class={CARD} data-part="password">
  <h2 class="font-semibold tracking-tight">{t('settings.password')}</h2>
  {#if auth.canChangePassword}
    <form class="grid gap-4" onsubmit={submit}>
      <p class="hint">{t('settings.password.hint')}</p>
      <div class="field">
        <label for="pw-current">{t('settings.password.current')}</label>
        <input
          id="pw-current"
          class="input"
          type="password"
          autocomplete="current-password"
          bind:value={current}
        />
      </div>
      <div class="field">
        <label for="pw-new">{t('settings.password.new')}</label>
        <input
          id="pw-new"
          class="input"
          type="password"
          autocomplete="new-password"
          bind:value={next}
          aria-invalid={submitted && problem ? 'true' : undefined}
          aria-describedby={submitted && problem ? 'pw-error' : undefined}
        />
        <p class="hint">{t('setup.password.hint', { min: MIN_PASSWORD_LEN })}</p>
      </div>
      <div class="field">
        <label for="pw-repeat">{t('settings.password.confirm')}</label>
        <input
          id="pw-repeat"
          class="input"
          type="password"
          autocomplete="new-password"
          bind:value={repeat}
          aria-invalid={submitted && problem === 'mismatch' ? 'true' : undefined}
        />
      </div>

      {#if submitted && problem}
        <p id="pw-error" class="text-sm text-danger" role="alert">{problemText}</p>
      {/if}
      {#if error}
        <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
          <Icon name="alert" size={18} />{error}
        </p>
      {/if}

      <button class="btn justify-self-start" type="submit" disabled={busy || !current}>
        {busy ? t('settings.password.changing') : t('settings.password.change')}
      </button>
    </form>
  {:else}
    <p class="hint">{t('settings.password.external')}</p>
  {/if}
</section>
