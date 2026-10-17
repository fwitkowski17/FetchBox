<script lang="ts">
  // "Version X is available": what changed since the running version (the release descriptions) and
  // how to update. The app cannot update itself (in a container the image is read-only), so it says
  // what to run on the machine. Opened after signing in (with "later" / "skip") or from Settings.
  import { t } from '../i18n';
  import { updateNotice } from '../stores/updateNotice.svelte';
  import { version } from '../stores/version.svelte';
  import { parseNotes } from '../util/notes';
  import { INSTALL_METHODS, packageSteps, repoOf, scriptStep } from '../util/updateSteps';
  import CommandBlock from './CommandBlock.svelte';
  import Modal from './Modal.svelte';
  import ReleaseNotes from './ReleaseNotes.svelte';

  const COMPOSE = ['git pull', 'docker compose up -d --build fetchbox'];
  const PLAIN = ['git pull', './scripts/build.sh', './target/release/fetchbox --data-dir ./data --download-dir ./downloads'];

  const info = $derived(version.info);
  const latest = $derived(info?.latest ?? null);
  const auto = $derived(updateNotice.mode === 'auto');
  const releaseUrl = $derived(info?.release_url?.startsWith('https://') ? info.release_url : null);

  // The installer states how FetchBox was installed; only those steps are shown. Without a
  // statement every way is listed, so nobody is left without instructions.
  const method = $derived(info?.install_method ?? null);
  const methods = $derived(method ? [method] : INSTALL_METHODS);
  const repo = $derived(repoOf(releaseUrl));
  const pkg = $derived(latest ? packageSteps(repo, latest) : null);
  const script = $derived(latest ? scriptStep(repo, latest, info?.latest_prerelease ?? false) : null);
</script>

<Modal
  open={updateNotice.open && info !== null && latest !== null}
  title={t('update.title', { version: latest ?? '' })}
  size="lg"
  onclose={() => updateNotice.dismiss(latest)}
>
  <div class="grid grid-cols-[minmax(0,1fr)] gap-4" data-part="update-dialog">
    <p>{t('update.intro', { current: info?.current ?? '' })}</p>
    {#if info?.latest_prerelease}
      <p class="hint" data-part="prerelease-note">
        {t(method === 'package' || method === 'script' ? 'update.prereleaseNotePinned' : 'update.prereleaseNote')}
      </p>
    {/if}

    <section class="grid grid-cols-[minmax(0,1fr)] gap-2">
      <h3 class="font-semibold">{t('update.whatsNew')}</h3>
      {#if info && info.notes.length > 0}
        <div class="max-h-64 overflow-y-auto rounded-lg border border-border bg-surface-2 p-3" data-part="update-notes">
          {#each info.notes as note (note.version)}
            <article class="grid gap-1 [&:not(:first-child)]:mt-4 [&:not(:first-child)]:border-t [&:not(:first-child)]:border-border [&:not(:first-child)]:pt-3">
              <h4 class="flex flex-wrap items-baseline justify-between gap-x-3 font-semibold">
                <span>
                  {t('update.version', { version: note.version })}
                  {#if note.prerelease}<span class="ml-1 rounded-full bg-accent-soft px-2 py-0.5 align-middle text-xs font-medium text-accent" data-part="prerelease-badge">{t('update.prerelease')}</span>{/if}
                </span>
                {#if note.url.startsWith('https://')}
                  <a class="text-sm font-normal" href={note.url} target="_blank" rel="noopener noreferrer">{t('update.github')}</a>
                {/if}
              </h4>
              {#if parseNotes(note.body).length > 0}
                <ReleaseNotes blocks={parseNotes(note.body)} />
              {:else}
                <p class="hint">{t('update.noNotes')}</p>
              {/if}
            </article>
          {/each}
        </div>
      {:else if releaseUrl}
        <p class="hint">
          {t('update.noNotes')} <a href={releaseUrl} target="_blank" rel="noopener noreferrer">{t('update.github')}</a>
        </p>
      {/if}
    </section>

    <section class="grid grid-cols-[minmax(0,1fr)] gap-2" data-part="update-how" data-method={method ?? 'unknown'}>
      <h3 class="font-semibold">{t('update.how')}</h3>
      {#if method === null}
        <p class="hint" data-part="update-unknown-method">{t('update.unknownMethod')}</p>
      {/if}
      {#each methods as m (m)}
        <div class="grid grid-cols-[minmax(0,1fr)] gap-2 [&:not(:first-of-type)]:mt-1" data-part="update-method" data-method={m}>
          {#if m === 'docker'}
            <h4 class="text-sm font-semibold">{t('update.compose')}</h4>
            <p class="hint">{t('update.composeSteps')}</p>
            {#each COMPOSE as command (command)}
              <CommandBlock {command} />
            {/each}
          {:else if m === 'package'}
            <h4 class="text-sm font-semibold">{t('update.package')}</h4>
            <p class="hint">{t('update.packageSteps')}</p>
            {#if pkg}
              <p class="text-sm font-medium">{t('update.packageDeb')}</p>
              {#each pkg.deb as command (command)}
                <CommandBlock {command} />
              {/each}
              <p class="text-sm font-medium">{t('update.packageRpm')}</p>
              {#each pkg.rpm as command (command)}
                <CommandBlock {command} />
              {/each}
            {/if}
          {:else if m === 'lxc'}
            <h4 class="text-sm font-semibold">{t('update.lxc')}</h4>
            <p class="hint">{t('update.lxcSteps')}</p>
            <CommandBlock command="update" />
          {:else if m === 'script'}
            <h4 class="text-sm font-semibold">{t('update.script')}</h4>
            <p class="hint">{t('update.scriptSteps')}</p>
            {#if script}
              <CommandBlock command={script} />
            {/if}
          {:else}
            <h4 class="text-sm font-semibold">{t('update.plain')}</h4>
            <p class="hint">{t('update.plainSteps')}</p>
            {#each PLAIN as command (command)}
              <CommandBlock {command} />
            {/each}
          {/if}
        </div>
      {/each}
      {#if method === null || method === 'docker' || method === 'lxc'}
        <p class="hint">{t('update.noSelf')}</p>
      {/if}
    </section>
  </div>

  {#snippet footer()}
    {#if auto && latest}
      <button class="btn" type="button" onclick={() => updateNotice.skip(latest)} data-part="update-skip">
        {t('update.skip')}
      </button>
      <button class="btn primary" type="button" onclick={() => updateNotice.remindLater(latest)} data-part="update-later">
        {t('update.later')}
      </button>
    {:else}
      <button class="btn primary" type="button" onclick={() => updateNotice.close()}>{t('common.close')}</button>
    {/if}
  {/snippet}
</Modal>
