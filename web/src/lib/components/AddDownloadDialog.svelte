<script lang="ts">
  import { ApiError, api, type SpaceInfo } from '../api/client';
  import type { Category, Conflict, OnConflict, UrlInfo } from '../api/types';
  import { CATEGORIES } from '../api/types';
  import ConflictDialog from './ConflictDialog.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';
  import NoSpaceDialog from './NoSpaceDialog.svelte';
  import { i18n, t } from '../i18n';
  import { addDialog } from '../stores/addDialog.svelte';
  import { toast } from '../stores/toast.svelte';
  import { isHttpUrl } from '../util/format';

  let url = $state('');
  let filename = $state('');
  let category = $state<Category | 'auto'>('auto');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let conflict = $state<Conflict | null>(null);
  /** Set while the user is asked about a file that does not fit; kept once they said "add anyway"
   *  so a conflict dialog after it does not ask again. */
  let noSpace = $state<SpaceInfo | null>(null);
  let ignoreSpace = false;

  const urlOk = $derived(isHttpUrl(url));
  const showUrlHint = $derived(url.trim() !== '' && !urlOk);

  /** What the server says about the address, shown under the field. Failing to look is never a
   *  reason to block the add (the add itself reports a real problem). */
  type Probe =
    | { state: 'idle' }
    | { state: 'checking' }
    | { state: 'ok'; info: UrlInfo }
    | { state: 'failed'; reason: string };
  let probe = $state<Probe>({ state: 'idle' });
  /** Typing waits a moment for more keystrokes; pasting an address looks at once. */
  const TYPING_DELAY_MS = 500;
  let lookAtOnce = false;

  $effect(() => {
    const address = url.trim();
    if (!addDialog.open || !isHttpUrl(address)) {
      probe = { state: 'idle' };
      return;
    }
    probe = { state: 'checking' };
    const abort = new AbortController();
    const delay = lookAtOnce ? 0 : TYPING_DELAY_MS;
    lookAtOnce = false;
    const timer = setTimeout(() => {
      api
        .probeUrl(address, abort.signal)
        .then((info) => {
          probe = { state: 'ok', info };
        })
        .catch((e: unknown) => {
          if (!abort.signal.aborted) probe = { state: 'failed', reason: i18n.errorText(e) };
        });
    }, delay);
    return () => {
      clearTimeout(timer);
      abort.abort();
    };
  });

  /** Reading the clipboard needs a secure context (HTTPS or localhost) and the user's consent;
   *  on a plain-HTTP LAN page it is not there, and Ctrl+V still works. */
  async function paste(): Promise<void> {
    let text: string;
    try {
      text = await navigator.clipboard.readText();
    } catch {
      toast.info(t('add.paste.unavailable'));
      document.getElementById('url')?.focus();
      return;
    }
    const first = text.trim().split(/\r?\n/)[0]?.trim() ?? '';
    if (first === '') {
      toast.info(t('add.paste.empty'));
      return;
    }
    error = null;
    lookAtOnce = true;
    url = first;
  }

  async function submit(onConflict?: OnConflict): Promise<void> {
    busy = true;
    error = null;
    try {
      const added = await api.addDownload({
        url: url.trim(),
        filename: filename.trim() || undefined,
        category: category === 'auto' ? undefined : category,
        on_conflict: onConflict,
        ignore_space: ignoreSpace || undefined,
      });
      conflict = null;
      ignoreSpace = false;
      if (added.waiting_space) toast.warning(t('add.held', { name: added.filename }));
      else toast.success(t('add.added', { name: added.filename }));
      url = '';
      filename = '';
      category = 'auto';
      addDialog.hide();
      location.hash = '#/';
    } catch (e) {
      if (e instanceof ApiError && e.status === 409 && e.conflict) {
        conflict = e.conflict; // the dialog lets the user choose, then we send again
      } else if (e instanceof ApiError && e.status === 507 && e.space) {
        conflict = null;
        noSpace = e.space; // the dialog explains the numbers and lets the user decide
      } else {
        conflict = null;
        error = e instanceof ApiError ? i18n.errorText(e) : t('add.failed');
      }
    } finally {
      busy = false;
    }
  }

  function onSubmit(e: SubmitEvent): void {
    e.preventDefault();
    if (!urlOk || busy) return;
    ignoreSpace = false; // a new attempt starts from the safe default
    void submit();
  }

  function chooseSpace(addAnyway: boolean): void {
    noSpace = null;
    if (addAnyway) {
      ignoreSpace = true;
      void submit();
    }
  }

  function choose(choice: OnConflict | null): void {
    if (choice === null) {
      conflict = null; // backing out simply does not send the request again
      ignoreSpace = false;
    } else {
      void submit(choice);
    }
  }

  /** Closing (button, Escape, backdrop) keeps what was typed but drops any pending question. */
  function close(): void {
    if (busy) return;
    addDialog.hide();
    error = null;
    conflict = null;
    noSpace = null;
    ignoreSpace = false;
  }
</script>

<Modal open={addDialog.open} title={t('add.title')} size="lg" onclose={close}>
  <form id="add-form" class="grid gap-4" onsubmit={onSubmit}>
    <div class="field">
      <label for="url">{t('add.address')}</label>
      <div class="relative">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          id="url"
          class="input pr-12"
          type="url"
          inputmode="url"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          autofocus
          placeholder="https://example.com/file.zip"
          bind:value={url}
          onpaste={() => (lookAtOnce = true)}
          aria-invalid={showUrlHint ? 'true' : undefined}
          aria-describedby="url-hint url-probe"
        />
        <button
          type="button"
          class="btn icon ghost absolute top-1/2 right-1 -translate-y-1/2"
          aria-label={t('add.paste')}
          title={t('add.paste')}
          data-part="paste"
          onclick={paste}
        >
          <Icon name="paste" size={20} />
        </button>
      </div>
      <p id="url-hint" class={['hint', showUrlHint && 'text-warning']}>
        {showUrlHint ? t('add.address.invalid') : t('add.address.hint')}
      </p>
      <div id="url-probe" class="min-h-6 text-sm" aria-live="polite" data-part="probe" data-state={probe.state}>
        {#if probe.state === 'checking'}
          <p class="text-muted">{t('add.probe.checking')}</p>
        {:else if probe.state === 'ok'}
          <p class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
            <CategoryIcon category={probe.info.category} size={18} />
            <span class="font-semibold tabular-nums" data-part="probe-size">
              {probe.info.size === null || probe.info.size === undefined
                ? t('add.probe.sizeUnknown')
                : t('add.probe.size', { size: i18n.bytes(probe.info.size) })}
            </span>
            {#if !probe.info.resumable}
              <span class="text-muted">{t('add.probe.noResume')}</span>
            {/if}
          </p>
        {:else if probe.state === 'failed'}
          <p class="line-clamp-3 text-warning [overflow-wrap:anywhere]">{t('add.probe.failed', { reason: probe.reason })}</p>
        {/if}
      </div>
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
          placeholder={probe.state === 'ok' ? probe.info.filename : t('add.filename.placeholder')}
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

  </form>

  {#snippet footer()}
    <button class="btn" type="button" disabled={busy} onclick={close}>{t('common.cancel')}</button>
    <button class="btn primary" type="submit" form="add-form" disabled={!urlOk || busy}>
      <Icon name="down" size={18} />{busy && !conflict ? t('add.submitting') : t('add.submit')}
    </button>
  {/snippet}
</Modal>

<ConflictDialog {conflict} {busy} onchoose={choose} />
<NoSpaceDialog space={noSpace} {busy} onchoose={chooseSpace} />
