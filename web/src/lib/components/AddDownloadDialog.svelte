<script lang="ts">
  import { ApiError, api, type SpaceInfo } from '../api/client';
  import type {
    Category,
    ChecksumAlgo,
    Conflict,
    HostKeyProblem,
    ListingEntry,
    OnConflict,
    UrlInfo,
  } from '../api/types';
  import { CATEGORIES } from '../api/types';
  import { untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import ConflictDialog from './ConflictDialog.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import HostKeyPrompt from './HostKeyPrompt.svelte';
  import Icon from './Icon.svelte';
  import ListingPicker from './ListingPicker.svelte';
  import Modal from './Modal.svelte';
  import NoSpaceDialog from './NoSpaceDialog.svelte';
  import { i18n, t } from '../i18n';
  import { addDialog } from '../stores/addDialog.svelte';
  import { settings } from '../stores/settings.svelte';
  import { toast } from '../stores/toast.svelte';
  import { ALGOS, algoLabel, checkChecksum, detectAlgo } from '../util/checksum';
  import { isSourceUrl, sourceOf } from '../util/format';
  import { emptySummary, tally, type BatchSummary } from '../util/listing';
  import { formatWhen, minutesToTime, nextOccurrence } from '../util/schedule';

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

  // ---- when to start (SPEC 6.3.2) ----------------------------------------------------
  /** The download hours are on: a new download waits for them unless "Download now" is chosen. */
  const hoursOn = $derived(settings.value?.schedule_enabled === true);
  let startNow = $state(false);
  /** No download hours: begin at a time of day instead of at once. */
  let startLater = $state(false);
  let startTime = $state('02:00');
  const startAt = $derived(startLater && !hoursOn ? nextOccurrence(startTime, new Date()) : null);

  /** What the add request says about the start. */
  function startFields(): { start_now?: boolean; start_at?: number } {
    if (hoursOn) return startNow ? { start_now: true } : {};
    return startAt ? { start_at: Math.floor(startAt.getTime() / 1000) } : {};
  }

  const urlOk = $derived(isSourceUrl(url));
  const showUrlHint = $derived(url.trim() !== '' && !urlOk);

  /** What the server says about the address, shown under the field. Failing to look is never a
   *  reason to block the add (the add itself reports a real problem). */
  type Probe =
    | { state: 'idle' }
    | { state: 'checking' }
    | { state: 'ok'; info: UrlInfo }
    /** An SFTP address needs its login before there is anything to look at. */
    | { state: 'needLogin' }
    /** The host key of an SSH server is unknown, or is not the accepted one. */
    | { state: 'hostkey'; problem: HostKeyProblem }
    | { state: 'failed'; reason: string };
  let probe = $state<Probe>({ state: 'idle' });
  const chosen = new SvelteSet<string>();
  /** Folders opened from a listing, so "Back" can return to them. */
  let trail = $state<string[]>([]);
  /** How far adding the chosen files has got (`done` of `total`), while it runs. */
  let progress = $state<{ done: number; total: number } | null>(null);
  /** The listing on screen. It stays while the next folder is being looked at, so the list does not
   *  collapse and spring back on every step. */
  let listing = $state<UrlInfo | null>(null);

  // ---- login: FTP, SFTP, HTTP basic authentication (SPEC 6.12) ---------------------------
  const source = $derived(sourceOf(url));
  let loginOn = $state(false);
  let username = $state('');
  let password = $state('');
  // SFTP has no anonymous login: its section is simply open.
  $effect(() => {
    if (source === 'sftp') untrack(() => (loginOn = true));
  });
  const login = $derived(
    loginOn && username.trim() !== '' ? { username: username.trim(), password } : undefined,
  );
  const loginReady = $derived(source !== 'sftp' || login !== undefined);
  /** FTP and HTTP without TLS send the password as it is: say so. */
  const plainLogin = $derived(/^\s*(ftp|http):/i.test(url));
  /** Asking the server again (after the user trusted its host key). */
  let probeAgain = $state(0);
  let trusting = $state(false);

  function resetLogin(): void {
    loginOn = false;
    username = '';
    password = '';
  }

  async function trustHost(problem: HostKeyProblem): Promise<void> {
    trusting = true;
    try {
      await api.trustHost({
        host: problem.host,
        port: problem.port,
        key_type: problem.key_type,
        fingerprint: problem.fingerprint,
      });
      lookAtOnce = true;
      probeAgain += 1;
    } catch (e) {
      toast.error(e instanceof ApiError ? i18n.errorText(e) : t('add.hostkey.failed'));
    } finally {
      trusting = false;
    }
  }

  function openSettings(): void {
    addDialog.hide();
    location.hash = '#/settings';
  }

  // ---- verify the finished file against a checksum (SPEC 6.2.2) ----------------------
  let verifyOn = $state(false);
  let sumAlgo = $state<ChecksumAlgo>('sha256');
  let sumValue = $state('');

  // Pasting a checksum picks its type: the label in it, or else its length. Choosing another type
  // by hand afterwards sticks until the text changes again.
  $effect(() => {
    const detected = detectAlgo(sumValue);
    if (detected !== null) untrack(() => (sumAlgo = detected));
  });

  /** What is wrong with the checksum so far (nothing is said about an empty field). */
  const sumProblem = $derived.by(() => {
    if (!verifyOn || sumValue.trim() === '') return null;
    const check = checkChecksum(sumAlgo, sumValue);
    if (check.ok) return null;
    if (check.reason === 'chars') return t('add.checksum.chars');
    if (check.reason === 'length') {
      return t('add.checksum.length', { algo: algoLabel(sumAlgo), expected: check.expected, got: check.got });
    }
    return null;
  });
  /** Nothing is asked of the checksum unless it is switched on (and a listing has one per file). */
  const sumReady = $derived(!verifyOn || listing !== null || checkChecksum(sumAlgo, sumValue).ok);

  /** What the add request says about the checksum. */
  function checksumFields(): { checksum?: { algo: ChecksumAlgo; value: string } } {
    return verifyOn && listing === null ? { checksum: { algo: sumAlgo, value: sumValue } } : {};
  }
  /** Typing waits a moment for more keystrokes; pasting an address looks at once. */
  const TYPING_DELAY_MS = 500;
  let lookAtOnce = false;

  $effect(() => {
    const address = url.trim();
    const credentials = login;
    void probeAgain; // asked again once the host key was trusted
    if (!addDialog.open || !isSourceUrl(address)) {
      probe = { state: 'idle' };
      listing = null;
      return;
    }
    if (!loginReady) {
      probe = { state: 'needLogin' };
      listing = null;
      return;
    }
    probe = { state: 'checking' };
    const abort = new AbortController();
    const delay = lookAtOnce ? 0 : TYPING_DELAY_MS;
    lookAtOnce = false;
    const timer = setTimeout(() => {
      api
        .probeUrl(address, credentials, abort.signal)
        .then((info) => {
          probe = { state: 'ok', info };
          listing = info.kind === 'listing' ? info : null;
          // A new listing starts with every file chosen: the user takes away what they do not want.
          chosen.clear();
          for (const e of info.entries) if (!e.is_dir) chosen.add(e.url);
        })
        .catch((e: unknown) => {
          if (abort.signal.aborted) return;
          listing = null;
          if (e instanceof ApiError && e.hostKey) {
            probe = { state: 'hostkey', problem: e.hostKey };
            return;
          }
          probe = { state: 'failed', reason: i18n.errorText(e) };
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
    trail = [];
    lookAtOnce = true;
    url = first;
  }

  function openFolder(folder: string): void {
    trail = [...trail, url];
    lookAtOnce = true;
    url = folder;
  }

  function goBack(): void {
    const previous = trail.at(-1);
    if (previous === undefined) return;
    trail = trail.slice(0, -1);
    lookAtOnce = true;
    url = previous;
  }

  /** Add every chosen file as its own download, one after the other (the server decides the names
   *  and checks space for each). Files that are already there or do not fit are skipped and
   *  counted, so one problem does not stop the rest. */
  async function submitChosen(entries: ListingEntry[]): Promise<void> {
    const files = entries.filter((e) => !e.is_dir && chosen.has(e.url));
    if (files.length === 0) return;
    busy = true;
    error = null;
    const summary: BatchSummary = emptySummary();
    for (const [i, file] of files.entries()) {
      progress = { done: i, total: files.length };
      try {
        await api.addDownload({
          url: file.url,
          category: category === 'auto' ? undefined : category,
          credentials: login,
          ...startFields(),
        });
        tally(summary, 'added');
      } catch (e) {
        if (e instanceof ApiError && e.status === 409) tally(summary, 'existing');
        else if (e instanceof ApiError && e.status === 507) tally(summary, 'noSpace');
        else tally(summary, 'failed', i18n.errorText(e));
      }
    }
    progress = null;
    busy = false;
    const notes = [
      summary.existing > 0 && t('add.listing.existing', { n: summary.existing }),
      summary.noSpace > 0 && t('add.listing.noSpace', { n: summary.noSpace }),
      summary.failed > 0 && t('add.listing.failed', { n: summary.failed }),
    ].filter((m): m is string => m !== false);
    if (summary.added === 0) {
      error = [t('add.listing.none'), ...notes, summary.firstError].filter(Boolean).join(' ');
      return;
    }
    const message = [t('add.listing.added', { n: summary.added }), ...notes].join('. ');
    if (notes.length > 0) toast.warning(message);
    else toast.success(message);
    url = '';
    trail = [];
    category = 'auto';
    startNow = false;
    startLater = false;
    resetLogin();
    addDialog.hide();
    location.hash = '#/';
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
        credentials: login,
        ...startFields(),
        ...checksumFields(),
      });
      conflict = null;
      ignoreSpace = false;
      if (added.waiting_space) toast.warning(t('add.held', { name: added.filename }));
      else toast.success(t('add.added', { name: added.filename }));
      url = '';
      filename = '';
      category = 'auto';
      startNow = false;
      startLater = false;
      verifyOn = false;
      sumValue = '';
      resetLogin();
      addDialog.hide();
      location.hash = '#/';
    } catch (e) {
      if (e instanceof ApiError && e.status === 409 && e.hostKey) {
        conflict = null;
        probe = { state: 'hostkey', problem: e.hostKey }; // shown under the address
      } else if (e instanceof ApiError && e.status === 409 && e.conflict) {
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
    if (listing) {
      void submitChosen(listing.entries);
      return;
    }
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
          oninput={() => (trail = [])}
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
        {showUrlHint ? t('add.address.invalid') : listing ? t('add.listing.title') : t('add.address.hint')}
      </p>
      <div id="url-probe" class={['text-sm', !listing && 'min-h-6']} aria-live="polite" data-part="probe" data-state={probe.state}>
        {#if probe.state === 'checking'}
          <p class="text-muted">{t('add.probe.checking')}</p>
        {:else if probe.state === 'ok' && probe.info.kind === 'file'}
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
        {:else if probe.state === 'needLogin'}
          <p class="text-muted" data-part="need-login">{t('add.login.needed')}</p>
        {:else if probe.state === 'hostkey'}
          <HostKeyPrompt
            problem={probe.problem}
            busy={trusting}
            ontrust={() => probe.state === 'hostkey' && trustHost(probe.problem)}
            onsettings={openSettings}
          />
        {:else if probe.state === 'failed'}
          <p class="line-clamp-3 text-warning [overflow-wrap:anywhere]">{t('add.probe.failed', { reason: probe.reason })}</p>
        {/if}
      </div>
    </div>

    <div class="grid gap-2" data-part="login-options">
      <label class="flex cursor-pointer items-center gap-3">
        <input
          type="checkbox"
          class="size-5 shrink-0 accent-[var(--accent)]"
          bind:checked={loginOn}
          disabled={source === 'sftp'}
          data-part="login"
        />
        <span class="font-semibold">{t('add.login')}</span>
      </label>
      {#if loginOn}
        <div class="grid items-start gap-3 pl-8 sm:grid-cols-2">
          <div class="field">
            <label for="login-user">{t('add.login.user')}</label>
            <input
              id="login-user"
              class="input"
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              bind:value={username}
              data-part="login-user"
            />
          </div>
          <div class="field">
            <label for="login-password">{t('add.login.password')}</label>
            <input
              id="login-password"
              class="input"
              type="password"
              autocomplete="new-password"
              bind:value={password}
              data-part="login-password"
            />
          </div>
          <p class="hint sm:col-span-2" data-part="login-hint">
            {t(`add.login.hint.${source ?? 'http'}`)} {t('add.login.stored')}
          </p>
          {#if plainLogin}
            <p class="flex items-start gap-2 text-sm text-warning sm:col-span-2" data-part="login-plain">
              <Icon name="alert" size={16} />{t('add.login.plain')}
            </p>
          {/if}
        </div>
      {/if}
    </div>

    {#if listing}
      <ListingPicker
        entries={listing.entries}
        truncated={listing.truncated}
        {chosen}
        canGoBack={trail.length > 0}
        disabled={busy || probe.state === 'checking'}
        onopen={openFolder}
        onback={goBack}
      />
    {/if}

    <div class={['grid gap-4', !listing && 'sm:grid-cols-2']}>
      {#if !listing}
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
      {/if}
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

    {#if !listing}
      <div class="grid gap-2" data-part="checksum-options">
        <label class="flex cursor-pointer items-center gap-3">
          <input
            type="checkbox"
            class="size-5 shrink-0 accent-[var(--accent)]"
            bind:checked={verifyOn}
            data-part="verify-checksum"
          />
          <span class="font-semibold">{t('add.checksum')}</span>
        </label>
        {#if verifyOn}
          <div class="grid items-start gap-3 pl-8 sm:grid-cols-[9rem_minmax(0,1fr)]">
            <div class="field">
              <label for="checksum-type">{t('add.checksum.type')}</label>
              <select id="checksum-type" class="select" bind:value={sumAlgo} data-part="checksum-type">
                {#each ALGOS as a (a.id)}
                  <option value={a.id}>{a.label}</option>
                {/each}
              </select>
            </div>
            <div class="field">
              <label for="checksum-value">{t('add.checksum.value')}</label>
              <input
                id="checksum-value"
                class="input font-mono text-sm"
                autocomplete="off"
                autocapitalize="off"
                spellcheck="false"
                placeholder={t('add.checksum.placeholder')}
                bind:value={sumValue}
                aria-invalid={sumProblem ? 'true' : undefined}
                aria-describedby="checksum-hint"
                data-part="checksum-value"
              />
              <p id="checksum-hint" class={['hint', sumProblem && 'text-warning']} data-part="checksum-hint">
                {sumProblem ?? t('add.checksum.hint')}
              </p>
            </div>
          </div>
        {/if}
      </div>
    {/if}

    <div class="grid gap-2" data-part="start-options">
      {#if hoursOn}
        <label class="flex cursor-pointer items-start gap-3">
          <input
            type="checkbox"
            class="mt-1 size-5 shrink-0 accent-[var(--accent)]"
            bind:checked={startNow}
            data-part="start-now"
          />
          <span class="grid gap-0.5">
            <span class="font-semibold">{t('add.schedule.now')}</span>
            <span class="hint">
              {t('add.schedule.nowHint', {
                from: minutesToTime(settings.value?.schedule_from ?? 0),
                to: minutesToTime(settings.value?.schedule_to ?? 0),
              })}
            </span>
          </span>
        </label>
      {:else}
        <label class="flex cursor-pointer items-center gap-3">
          <input
            type="checkbox"
            class="size-5 shrink-0 accent-[var(--accent)]"
            bind:checked={startLater}
            data-part="start-later"
          />
          <span class="font-semibold">{t('add.schedule.later')}</span>
        </label>
        {#if startLater}
          <div class="flex flex-wrap items-center gap-x-4 gap-y-1 pl-8">
            <input
              class="input w-32"
              type="time"
              required
              aria-label={t('add.schedule.at')}
              bind:value={startTime}
              data-part="start-time"
            />
            {#if startAt}
              <span class="hint" data-part="start-when">
                {t('add.schedule.when', { when: formatWhen(Math.floor(startAt.getTime() / 1000), i18n.locale) })}
              </span>
            {/if}
          </div>
        {/if}
      {/if}
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
    <button
      class="btn primary"
      type="submit"
      form="add-form"
      disabled={!urlOk ||
        busy ||
        !sumReady ||
        !loginReady ||
        probe.state === 'hostkey' ||
        (startLater && !hoursOn && startAt === null) ||
        (listing !== null && (chosen.size === 0 || probe.state === 'checking'))}
    >
      <Icon name="down" size={18} />
      {#if progress}
        {t('add.listing.progress', { done: progress.done + 1, total: progress.total })}
      {:else if listing}
        {t('add.listing.submit', { n: chosen.size })}
      {:else}
        {busy && !conflict ? t('add.submitting') : t('add.submit')}
      {/if}
    </button>
  {/snippet}
</Modal>

<ConflictDialog {conflict} {busy} onchoose={choose} />
<NoSpaceDialog space={noSpace} {busy} onchoose={chooseSpace} />
