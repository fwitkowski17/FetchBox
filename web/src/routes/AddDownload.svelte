<script lang="ts">
  import { ApiError, api } from '../lib/api/client';
  import type { Category, Conflict, OnConflict } from '../lib/api/types';
  import { CATEGORIES } from '../lib/api/types';
  import ConflictDialog from '../lib/components/ConflictDialog.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { i18n, t } from '../lib/i18n';
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
      toast.success(t('add.added', { name: added.filename }));
      url = '';
      filename = '';
      category = 'auto';
      location.hash = '#/';
    } catch (e) {
      if (e instanceof ApiError && e.status === 409 && e.conflict) {
        conflict = e.conflict; // the dialog lets the user choose, then we send again
      } else {
        conflict = null;
        error =
          e instanceof ApiError && e.status === 507 && e.space
            ? t('add.noSpace', {
                needed: i18n.bytes(e.space.needed),
                available: i18n.bytes(e.space.available),
              })
            : e instanceof ApiError
              ? i18n.errorText(e)
              : t('add.failed');
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

<div class="page-narrow">
  <header class="mb-4"><h1 class="text-2xl font-bold tracking-tight">{t('add.title')}</h1></header>

  <form
    class="grid gap-4 rounded-2xl border border-border bg-surface p-5 shadow-card"
    onsubmit={onSubmit}
  >
    <div class="field">
      <label for="url">{t('add.address')}</label>
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
      <p id="url-hint" class={['hint', showUrlHint && 'text-warning']}>
        {showUrlHint ? t('add.address.invalid') : t('add.address.hint')}
      </p>
    </div>

    <div class="grid gap-4 sm:grid-cols-2">
      <div class="field">
        <label for="filename">
          {t('add.filename')} <span class="font-normal text-muted">{t('common.optional')}</span>
        </label>
        <input
          id="filename"
          class="input"
          autocomplete="off"
          placeholder={t('add.filename.placeholder')}
          bind:value={filename}
        />
      </div>
      <div class="field">
        <label for="category">{t('add.category')}</label>
        <select id="category" class="select" bind:value={category}>
          <option value="auto">{t('add.category.auto')}</option>
          {#each CATEGORIES as c (c)}
            <option value={c}>{t(`category.${c}`)}</option>
          {/each}
        </select>
      </div>
    </div>

    {#if error}
      <p
        class="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger [overflow-wrap:anywhere]"
        role="alert"
      >
        <Icon name="alert" size={18} />{error}
      </p>
    {/if}

    <button class="btn primary" type="submit" disabled={!urlOk || busy}>
      <Icon name="down" size={18} />{busy && !conflict ? t('add.submitting') : t('add.submit')}
    </button>
  </form>
</div>

<ConflictDialog {conflict} {busy} onchoose={choose} />
