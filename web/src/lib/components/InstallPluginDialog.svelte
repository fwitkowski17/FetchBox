<script lang="ts">
  // Shows what an unpacked plugin is and what it asks for. Nothing is installed (and nothing
  // runs) until the user presses the button.
  import { i18n, t } from '../i18n';
  import { installPlugin } from '../stores/installPlugin.svelte';
  import { dependencyReview } from '../util/dependencies';
  import { formatBytes } from '../util/format';
  import { isSafeExternalUrl } from '../util/pluginUi';
  import DependencyList from './DependencyList.svelte';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';
  import PermissionList from './PermissionList.svelte';

  const staged = $derived(installPlugin.staged);
  const update = $derived(staged?.replaces ?? null);
  // The dialog keeps showing the last plugin while it plays its exit.
  let shown = $state.raw(installPlugin.staged);
  $effect(() => {
    if (staged) shown = staged;
  });

  const deps = $derived(shown ? dependencyReview(shown) : null);
  // The user's yes to the downloads; it never carries over from one plugin to the next.
  let agreed = $state(false);
  $effect(() => {
    void staged?.token;
    agreed = false;
  });
  const needsAgreement = $derived(!!deps?.confirm && !agreed);

  /** The total of what is downloaded: "about 30 MB", "at least 30 MB" when some sizes are not known. */
  const total = $derived.by(() => {
    if (!deps) return '';
    if (deps.bytes === 0) return t('ext.install.depsTotalUnknown');
    const size = formatBytes(deps.bytes, i18n.locale);
    return t('ext.install.depsTotal', {
      size: t(deps.unknownSizes > 0 ? 'ext.install.depsSizeAtLeast' : 'ext.install.depsSize', { size }),
    });
  });
</script>

<Modal
  open={installPlugin.open}
  title={shown ? (update ? t('ext.install.updateTitle', { name: shown.name }) : t('ext.install.title', { name: shown.name })) : ''}
  onclose={() => void installPlugin.cancel()}
  size="lg"
>
  {#if shown}
    <div class="grid gap-4" data-part="install-plugin">
      <div>
        <p class="font-semibold">
          {shown.name}
          <span class="font-normal text-muted">
            {#if update}{update.version} → {shown.version}{:else}{shown.version}{/if}
          </span>
        </p>
        <p class="hint">
          {[t('ext.by', { author: shown.author }), t(`ext.source.${shown.source}`), shown.license].filter(Boolean).join(' · ')}
        </p>
        {#if shown.description}<p class="mt-1">{shown.description}</p>{/if}
        {#if isSafeExternalUrl(shown.homepage)}
          <a class="text-sm" href={shown.homepage} target="_blank" rel="noopener noreferrer">{t('ext.homepage')}</a>
        {/if}
      </div>

      <p class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" role="note">
        <Icon name="alert" size={18} />{t('ext.install.warning')}
      </p>

      <section class="grid gap-2">
        <h3 class="font-semibold">{update ? t('ext.install.permissionsUpdate') : t('ext.install.permissions')}</h3>
        <PermissionList permissions={shown.permissions} previous={update?.permissions ?? null} />
      </section>

      {#if deps && deps.rows.length > 0}
        <section class="grid gap-2" data-part="install-dependencies">
          <h3 class="font-semibold">{update ? t('ext.install.dependenciesUpdate') : t('ext.install.dependencies')}</h3>
          <p class="hint">{t('ext.install.dependencies.about')}</p>
          <DependencyList review={deps} />
          {#if deps.confirm}
            <label class="flex cursor-pointer items-start gap-3 rounded-lg bg-surface-2 px-3 py-2.5 text-sm">
              <input
                type="checkbox"
                class="mt-0.5 size-5 shrink-0 accent-[var(--accent)]"
                bind:checked={agreed}
                disabled={installPlugin.busy}
                data-part="install-dependencies-agree"
              />
              <span>
                {t('ext.install.depsConfirm', { n: deps.downloads.length })}
                <span class="hint block">{total}</span>
                <span class="hint block">{deps.when === 'now' ? t('ext.install.depsNow') : t('ext.install.depsWhenOn')}</span>
              </span>
            </label>
          {:else}
            <p class="hint">{t('ext.install.depsNothing')}</p>
          {/if}
        </section>
      {/if}

      {#if installPlugin.error}
        <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger" role="alert">
          <Icon name="alert" size={18} />{installPlugin.error}
        </p>
      {/if}
    </div>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn" onclick={() => void installPlugin.cancel()} disabled={installPlugin.busy}>
      {t('common.cancel')}
    </button>
    <button
      type="button"
      class="btn primary"
      onclick={() => void installPlugin.install()}
      disabled={installPlugin.busy || !installPlugin.staged || needsAgreement}
      data-part="install-plugin-confirm"
    >
      {installPlugin.busy ? t('ext.install.installing') : update ? t('ext.install.update') : t('ext.install.install')}
    </button>
  {/snippet}
</Modal>
