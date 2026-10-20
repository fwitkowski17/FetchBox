<script lang="ts">
  // One installed plugin on the Extensions page: what it is, its state, its dependencies and the
  // few things the user does with it (switch on or off, approve, update, restart, remove).
  import { api } from '../api/client';
  import type { PluginView } from '../api/types';
  import { i18n, t } from '../i18n';
  import { confirmAction } from '../stores/confirm.svelte';
  import { installPlugin } from '../stores/installPlugin.svelte';
  import { plugins } from '../stores/plugins.svelte';
  import { toast } from '../stores/toast.svelte';
  import { CARD } from '../util/classes';
  import { cannotEnable, dependencyPercent, isUsable } from '../util/plugins';
  import { iconOrDefault, isSafeExternalUrl, tr } from '../util/pluginUi';
  import Icon, { ICON_NAMES } from './Icon.svelte';
  import Modal from './Modal.svelte';
  import PermissionList from './PermissionList.svelte';
  import PluginActions from './PluginActions.svelte';
  import PluginStateBadge from './PluginStateBadge.svelte';
  import Toggle from './Toggle.svelte';

  let { plugin }: { plugin: PluginView } = $props();

  let busy = $state(false);
  let reviewing = $state(false);
  let logs = $state<string[] | null>(null);
  let logsError = $state<string | null>(null);

  const L = (text: string) => tr(plugin.i18n, i18n.locale, text);
  const blocked = $derived(cannotEnable(plugin));
  const percent = $derived(dependencyPercent(plugin));
  const missing = $derived(plugin.dependencies.filter((d) => !d.installed && !d.optional));
  const cardActions = $derived(plugin.ui.actions.filter((a) => a.placement === 'card'));

  async function toggle(enabled: boolean): Promise<void> {
    busy = true;
    await plugins.setEnabled(plugin.id, enabled);
    busy = false;
  }

  async function approve(): Promise<void> {
    busy = true;
    const ok = await plugins.approve(plugin.id, plugin.manifest_hash);
    busy = false;
    if (ok) reviewing = false;
  }

  async function update(): Promise<void> {
    await installPlugin.stageFromStore(plugin.id);
  }

  async function remove(): Promise<void> {
    const ok = await confirmAction({
      title: t('ext.remove.title', { name: plugin.name }),
      message: t('ext.remove.message'),
      confirmLabel: t('ext.remove.confirm'),
      danger: true,
    });
    if (!ok) return;
    busy = true;
    if (await plugins.remove(plugin.id, false)) toast.success(t('ext.remove.done', { name: plugin.name }));
    busy = false;
  }

  async function showLogs(): Promise<void> {
    if (logs !== null) {
      logs = null;
      return;
    }
    logsError = null;
    try {
      logs = (await api.pluginLogs(plugin.id)).lines;
    } catch (e) {
      logsError = i18n.errorText(e);
    }
  }
</script>

<article class={CARD} data-part="plugin-card" data-plugin={plugin.id} data-state={plugin.state}>
  <header class="flex items-start gap-3">
    <span class="mt-0.5 text-muted"><Icon name={iconOrDefault(plugin.ui.icon, ICON_NAMES, 'puzzle')} size={24} /></span>
    <div class="min-w-0 flex-1">
      <h2 class="flex flex-wrap items-center gap-x-2 gap-y-1 font-semibold tracking-tight">
        <span class="[overflow-wrap:anywhere]">{plugin.name}</span>
        <span class="text-sm font-normal text-muted">{plugin.version}</span>
        <PluginStateBadge state={plugin.state} />
      </h2>
      <p class="hint">
        {t('ext.by', { author: plugin.author })} · {t(`ext.source.${plugin.source}`)}{#if plugin.license} · {plugin.license}{/if}
        {#if isSafeExternalUrl(plugin.homepage)}
          · <a href={plugin.homepage} target="_blank" rel="noopener noreferrer">{t('ext.homepage')}</a>
        {/if}
      </p>
    </div>
  </header>

  {#if plugin.description}<p class="[overflow-wrap:anywhere]">{L(plugin.description)}</p>{/if}

  {#if plugin.error}
    <p class="flex gap-2 rounded-lg bg-danger-soft px-3 py-2 text-danger [overflow-wrap:anywhere]" role="alert">
      <Icon name="alert" size={18} />{plugin.error}
    </p>
  {/if}
  {#if plugin.status_text}<p class="hint [overflow-wrap:anywhere]">{plugin.status_text}</p>{/if}

  {#if plugin.state === 'needs_review'}
    <div class="flex flex-wrap items-center gap-2 rounded-lg bg-warning-soft px-3 py-2 text-warning">
      <Icon name="alert" size={18} />
      <span class="min-w-0 flex-1 text-sm">{t('ext.review.hint')}</span>
      <button type="button" class="btn" onclick={() => (reviewing = true)} data-part="plugin-review">{t('ext.review.button')}</button>
    </div>
  {/if}

  {#if plugin.dependencies.length > 0}
    <section class="grid gap-1.5" aria-label={t('ext.deps.title')}>
      <h3 class="text-sm font-semibold">{t('ext.deps.title')}</h3>
      <ul class="grid gap-1">
        {#each plugin.dependencies as d (d.id)}
          <li class="flex items-center gap-2 text-sm" data-dependency={d.id}>
            <Icon name={d.installed ? 'checkCircle' : 'down'} size={16} />
            <span class="min-w-0 flex-1 [overflow-wrap:anywhere]">
              {d.label}{#if d.version} <span class="text-muted">{d.version}</span>{/if}{#if d.optional} <span class="text-muted">({t('ext.deps.optional')})</span>{/if}
            </span>
            <span class="text-muted">{d.installed ? t('ext.deps.installed') : t('ext.deps.missing')}</span>
          </li>
        {/each}
      </ul>
      {#if plugin.progress}
        <div
          class="h-1.5 overflow-hidden rounded-full bg-surface-2"
          role="progressbar"
          aria-label={t('ext.deps.fetching', { name: plugin.progress.dependency })}
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={percent ?? undefined}
        >
          <div
            class={['h-full bg-accent transition-[width]', percent === null && 'w-1/3 animate-pulse']}
            style:width={percent === null ? undefined : `${percent}%`}
          ></div>
        </div>
        <p class="hint">{t('ext.deps.fetching', { name: plugin.progress.dependency })}{#if percent !== null} · {percent}%{/if}</p>
      {:else if missing.length > 0 && !blocked}
        <div>
          <button type="button" class="btn" onclick={() => void plugins.fetchDependencies(plugin.id)} data-part="plugin-fetch-deps">
            <Icon name="down" size={18} />{t('ext.deps.fetch')}
          </button>
        </div>
      {/if}
    </section>
  {/if}

  <div class="flex flex-wrap items-center gap-2">
    <div class="min-w-0 flex-1 sm:flex-none">
      <Toggle
        bind:checked={() => plugin.enabled, (v) => void toggle(v)}
        disabled={busy || blocked !== null}
        hint={blocked ? t(`ext.cannotEnable.${blocked}`) : undefined}
      >
        {t('ext.enabled')}
      </Toggle>
    </div>
    <span class="flex-1"></span>
    {#if plugin.update_available}
      <button type="button" class="btn primary" onclick={() => void update()} data-part="plugin-update">
        <Icon name="up" size={18} />{t('ext.update', { version: plugin.update_available })}
      </button>
    {/if}
    {#if isUsable(plugin)}
      <PluginActions {plugin} actions={cardActions} />
    {/if}
    {#if plugin.provides.process && plugin.enabled && plugin.state !== 'needs_review'}
      <button type="button" class="btn" onclick={() => void plugins.restart(plugin.id)}>
        <Icon name="retry" size={18} />{t('ext.restart')}
      </button>
    {/if}
    {#if plugin.state !== 'unsupported'}
      <button type="button" class="btn ghost" onclick={() => void showLogs()} aria-expanded={logs !== null}>
        {logs !== null ? t('ext.logs.hide') : t('ext.logs.show')}
      </button>
    {/if}
    <button type="button" class="btn danger" onclick={() => void remove()} disabled={busy} data-part="plugin-remove">
      <Icon name="trash" size={18} />{t('ext.remove')}
    </button>
  </div>

  {#if logsError}<p class="text-sm text-danger" role="alert">{logsError}</p>{/if}
  {#if logs !== null}
    <pre class="max-h-64 overflow-auto rounded-lg bg-surface-2 p-3 font-mono text-xs [overflow-wrap:anywhere] whitespace-pre-wrap" data-part="plugin-logs">{logs.length > 0 ? logs.join('\n') : t('ext.logs.empty')}</pre>
  {/if}
</article>

<Modal open={reviewing} title={t('ext.review.title', { name: plugin.name })} onclose={() => (reviewing = false)} size="lg">
  <div class="grid gap-4">
    <p>{t('ext.review.intro')}</p>
    <p class="flex gap-2 rounded-lg bg-warning-soft px-3 py-2 text-sm text-warning" role="note">
      <Icon name="alert" size={18} />{t('ext.install.warning')}
    </p>
    <PermissionList permissions={plugin.permissions} />
  </div>
  {#snippet footer()}
    <button type="button" class="btn" onclick={() => (reviewing = false)}>{t('common.cancel')}</button>
    <button type="button" class="btn primary" onclick={() => void approve()} disabled={busy} data-part="plugin-approve">
      {t('ext.review.approve')}
    </button>
  {/snippet}
</Modal>
