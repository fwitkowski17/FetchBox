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
      } else {
        error = err instanceof ApiError && err.status === 0 ? i18n.errorText(err) : t('login.failed');
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="wrap">
  <form class="card" onsubmit={submit}>
    <div class="logo"><Icon name="lock" size={26} /></div>
    <h1>dlmanager</h1>
    <p class="hint">{t('login.prompt')}</p>

    <div class="field">
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

    {#if error}<p id="login-error" class="error" role="alert">{error}</p>{/if}

    <button class="btn primary" type="submit" disabled={busy || !password}>
      {busy ? t('login.submitting') : t('login.submit')}
    </button>
  </form>
</div>

<style>
  .wrap {
    display: grid;
    place-items: center;
    min-height: 100dvh;
    padding: 1rem;
  }

  .card {
    display: grid;
    gap: 1rem;
    width: min(24rem, 100%);
    padding: 1.5rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    text-align: center;
  }

  .logo {
    display: grid;
    place-items: center;
    width: 3.25rem;
    height: 3.25rem;
    margin: 0 auto;
    border-radius: 14px;
    background: var(--accent-soft);
    color: var(--accent);
  }

  .field {
    text-align: left;
  }

  .error {
    color: var(--danger);
    font-size: 0.9rem;
  }
</style>
