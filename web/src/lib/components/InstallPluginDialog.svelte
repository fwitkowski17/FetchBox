<script lang="ts">
  // Shows what an unpacked plugin is and what it asks for. Nothing is installed (and nothing
  // runs) until the user presses the button.
  import { t } from '../i18n';
  import { installPlugin } from '../stores/installPlugin.svelte';
  import { isSafeExternalUrl } from '../util/pluginUi';
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
      disabled={installPlugin.busy || !installPlugin.staged}
      data-part="install-plugin-confirm"
    >
      {installPlugin.busy ? t('ext.install.installing') : update ? t('ext.install.update') : t('ext.install.install')}
    </button>
  {/snippet}
</Modal>
