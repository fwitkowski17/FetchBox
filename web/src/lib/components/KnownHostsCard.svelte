<script lang="ts">
  // The "Trusted SSH servers" card of Settings: the servers whose host key was accepted, each
  // with a button to forget it (SFTP downloads from it stop until it is trusted again).
  import { onMount } from 'svelte';
  import { i18n, t } from '../i18n';
  import { confirmAction } from '../stores/confirm.svelte';
  import { knownHosts } from '../stores/knownHosts.svelte';
  import { toast } from '../stores/toast.svelte';
  import { CARD } from '../util/classes';
  import Icon from './Icon.svelte';
  import type { KnownHost } from '../api/types';

  onMount(() => void knownHosts.load());

  async function forget(host: KnownHost): Promise<void> {
    const ok = await confirmAction({
      title: t('settings.hosts.confirm.title'),
      message: t('settings.hosts.confirm.message', { host: host.host, port: host.port }),
      confirmLabel: t('settings.hosts.forget'),
      danger: true,
    });
    if (!ok) return;
    try {
      await knownHosts.forget(host.id);
      toast.success(t('settings.hosts.forgotten'));
    } catch (e) {
      toast.error(i18n.errorText(e));
    }
  }
</script>

<section class={CARD} data-part="known-hosts">
  <h2 class="font-semibold tracking-tight">{t('settings.hosts')}</h2>
  <p class="hint">{t('settings.hosts.hint')}</p>
  {#if knownHosts.error && knownHosts.list === null}
    <p class="text-warning" role="alert">
      {t('settings.hosts.failed', { error: i18n.errorText(knownHosts.error) })}
    </p>
  {:else if knownHosts.list === null}
    <p class="hint" role="status">{t('common.loading')}</p>
  {:else if knownHosts.list.length === 0}
    <p class="hint" data-part="known-hosts-empty">{t('settings.hosts.empty')}</p>
  {:else}
    <ul class="grid gap-3">
      {#each knownHosts.list as host (host.id)}
        <li class="flex items-start gap-3" data-part="known-host">
          <span class="mt-0.5 text-muted"><Icon name="server" size={20} /></span>
          <div class="min-w-0 flex-1">
            <p class="font-semibold [overflow-wrap:anywhere]">{host.host}<span class="font-normal text-muted">:{host.port}</span></p>
            <code class="block font-mono text-xs text-muted [overflow-wrap:anywhere]">{host.key_type} {host.fingerprint}</code>
            <p class="hint">{t('settings.hosts.since', { date: i18n.date(host.added_at) })}</p>
          </div>
          <button
            type="button"
            class="btn danger shrink-0"
            aria-label={t('settings.hosts.forgetLabel', { host: host.host })}
            onclick={() => forget(host)}
          >
            <Icon name="trash" size={18} />{t('settings.hosts.forget')}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
