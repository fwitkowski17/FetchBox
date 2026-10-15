<script lang="ts">
  // Change the password of an account created in the UI: the current one, the new one, the new one
  // again. Hidden text only; `autocomplete` hints let a password manager update its entry. The
  // server ends every other session (this one stays).
  import { ApiError } from '../api/client';
  import { i18n, t } from '../i18n';
  import { auth } from '../stores/auth.svelte';
  import { toast } from '../stores/toast.svelte';
  import { MIN_PASSWORD_LEN, checkPasswordChange } from '../util/password';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  interface Props {
    open: boolean;
    onclose: () => void;
  }

  let { open, onclose }: Props = $props();

  let current = $state('');
  let next = $state('');
  let repeat = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** The checks speak up after the first try to submit (a mismatch also while repeating). */
  let submitted = $state(false);

  const problem = $derived(checkPasswordChange(current, next, repeat));
  const shown = $derived(
    submitted ? problem : repeat !== '' && next !== repeat ? ('mismatch' as const) : null,
  );
  const problemText = $derived(
    shown === 'mismatch'
      ? t('setup.error.mismatch')
      : shown === 'same'
        ? t('settings.password.same')
        : shown
          ? t('setup.error.short', { min: MIN_PASSWORD_LEN })
          : '',
  );

  // Every opening starts empty: a password must not wait in the page for the next visitor.
  $effect(() => {
    if (open) {
      current = next = repeat = '';
      error = null;
      submitted = false;
    }
  });

  function close(): void {
    if (busy) return;
    current = next = repeat = '';
    onclose();
  }

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    submitted = true;
    if (busy || !current || problem) return;
    busy = true;
    error = null;
    try {
      await auth.changePassword(current, next);
      toast.success(t('settings.password.changed'));
      current = next = repeat = '';
      onclose(); // not `close()`: that waits for the request, which is still "busy" here
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

<Modal {open} title={t('settings.password.title')} onclose={close}>
  <form id="change-password-form" class="grid gap-4" onsubmit={submit} data-part="change-password">
    <p class="hint">{t('settings.password.hint')}</p>
    <div class="field">
      <label for="pw-current">{t('settings.password.current')}</label>
      <input
        id="pw-current"
        class="input"
        type="password"
        autocomplete="current-password"
        bind:value={current}
        aria-invalid={error === t('settings.password.wrong') ? 'true' : undefined}
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
        aria-invalid={shown && shown !== 'mismatch' ? 'true' : undefined}
        aria-describedby={shown ? 'pw-error' : undefined}
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
        aria-invalid={shown === 'mismatch' ? 'true' : undefined}
        aria-describedby={shown ? 'pw-error' : undefined}
      />
    </div>

    {#if shown}
      <p id="pw-error" class="text-sm text-danger" role="alert">{problemText}</p>
    {/if}
    {#if error}
      <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
        <Icon name="alert" size={18} />{error}
      </p>
    {/if}
  </form>

  {#snippet footer()}
    <button class="btn" type="button" disabled={busy} onclick={close}>{t('common.cancel')}</button>
    <button class="btn primary" type="submit" form="change-password-form" disabled={busy || !current}>
      {busy ? t('settings.password.changing') : t('settings.password.change')}
    </button>
  {/snippet}
</Modal>
