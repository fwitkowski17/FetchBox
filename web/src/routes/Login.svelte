<script lang="ts">
  import { ApiError } from '../lib/api/client';
  import Icon from '../lib/components/Icon.svelte';
  import { i18n, t } from '../lib/i18n';
  import { auth } from '../lib/stores/auth.svelte';

  let password = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (busy || !password) return;
    busy = true;
    error = null;
    try {
      await auth.login(password);
      password = '';
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        error = t('login.wrong');
      } else if (err instanceof ApiError && err.status === 429) {
        error = t('login.locked', { n: err.retryAfter ?? 60 });
      } else if (err instanceof ApiError && err.status === 0) {
        void auth.check(); // the server is gone: the connection dialog takes over
        error = i18n.errorText(err);
      } else {
        error = t('login.failed');
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="grid min-h-dvh place-items-center p-4">
  <form
    class="grid w-[min(24rem,100%)] gap-4 rounded-2xl border border-border bg-surface p-6 text-center shadow-card"
    onsubmit={submit}
  >
    <div class="mx-auto grid size-14 place-items-center rounded-2xl bg-accent text-accent-contrast">
      <Icon name="lock" size={26} />
    </div>
    <div>
      <h1 class="text-2xl font-bold tracking-tight">FetchBox</h1>
      <p class="hint mt-1">{t('login.prompt')}</p>
    </div>

    <div class="field text-left">
      <label for="password">{t('login.password')}</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        id="password"
        class="input"
        type="password"
        autocomplete="current-password"
        autofocus
        bind:value={password}
        aria-invalid={error ? 'true' : undefined}
        aria-describedby={error ? 'login-error' : undefined}
      />
    </div>

    {#if error}<p id="login-error" class="text-sm text-danger" role="alert">{error}</p>{/if}

    <button class="btn primary" type="submit" disabled={busy || !password}>
      {busy ? t('login.submitting') : t('login.submit')}
    </button>
  </form>
</div>
