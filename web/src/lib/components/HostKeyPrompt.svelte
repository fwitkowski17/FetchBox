<script lang="ts">
  // The host key of an SSH server is unknown (the user compares the fingerprint and decides to
  // trust it) or is not the one that was accepted (a warning; nothing is sent).
  import type { HostKeyProblem } from '../api/types';
  import { t } from '../i18n';
  import Icon from './Icon.svelte';

  interface Props {
    problem: HostKeyProblem;
    busy: boolean;
    ontrust: () => void;
    onsettings: () => void;
  }

  let { problem, busy, ontrust, onsettings }: Props = $props();

  const changed = $derived(problem.kind === 'changed');

  /** The file that holds the key on the server, for the hint on how to compare. */
  const keyFile = $derived.by(() => {
    const type = problem.key_type;
    if (type.includes('ed25519')) return 'ssh_host_ed25519_key.pub';
    if (type.includes('rsa')) return 'ssh_host_rsa_key.pub';
    if (type.includes('ecdsa')) return 'ssh_host_ecdsa_key.pub';
    return 'ssh_host_*_key.pub';
  });
</script>

<div
  class={[
    'grid gap-2 rounded-lg px-3 py-3 text-sm',
    changed ? 'bg-danger-soft' : 'border border-border bg-surface-2',
  ]}
  role={changed ? 'alert' : 'group'}
  aria-label={changed ? t('add.hostkey.changed.title') : t('add.hostkey.unknown.title')}
  data-part="hostkey"
  data-kind={problem.kind}
>
  <p class={['flex items-center gap-2 font-semibold', changed ? 'text-danger' : '']}>
    <Icon name={changed ? 'alert' : 'key'} size={18} />
    {changed ? t('add.hostkey.changed.title') : t('add.hostkey.unknown.title')}
  </p>
  <p class="[overflow-wrap:anywhere]">
    {t(changed ? 'add.hostkey.changed' : 'add.hostkey.unknown', {
      host: problem.host,
      port: problem.port,
    })}
  </p>
  {#if changed && problem.trusted_fingerprint}
    <div>
      <p class="hint">{t('add.hostkey.accepted')}</p>
      <code class="block font-mono text-xs [overflow-wrap:anywhere]" data-part="hostkey-old">{problem.trusted_fingerprint}</code>
    </div>
  {/if}
  <div>
    <p class="hint">
      {changed ? t('add.hostkey.now') : t('add.hostkey.fingerprint', { type: problem.key_type })}
    </p>
    <code class="block font-mono text-sm [overflow-wrap:anywhere]" data-part="hostkey-fingerprint">{problem.fingerprint}</code>
  </div>
  {#if changed}
    <p class="hint">{t('add.hostkey.settingsHint')}</p>
    <button type="button" class="btn justify-self-start" onclick={onsettings} data-part="hostkey-settings">
      <Icon name="sliders" size={18} />{t('add.hostkey.settings')}
    </button>
  {:else}
    <p class="hint">{t('add.hostkey.how', { file: keyFile })}</p>
    <button
      type="button"
      class="btn primary justify-self-start"
      disabled={busy}
      onclick={ontrust}
      data-part="hostkey-trust"
    >
      <Icon name="check" size={18} />{busy ? t('add.hostkey.trusting') : t('add.hostkey.trust')}
    </button>
  {/if}
</div>
