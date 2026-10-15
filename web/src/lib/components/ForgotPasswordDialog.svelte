<script lang="ts">
  // "Forgot your password?" on the login screen. There is no e-mail reset (the server only keeps a
  // hash), so this says how to reset it on the machine that runs the server: `fetchbox reset-password`
  // forgets the password, and the next start asks for a new account with a setup code in its log.
  import { t } from '../i18n';
  import { auth } from '../stores/auth.svelte';
  import CommandBlock from './CommandBlock.svelte';
  import Modal from './Modal.svelte';

  interface Props {
    open: boolean;
    onclose: () => void;
  }

  let { open, onclose }: Props = $props();

  const DOCKER = [
    'docker compose stop fetchbox',
    'docker compose run --rm fetchbox reset-password',
    'docker compose up -d fetchbox',
    'docker compose logs fetchbox',
  ];
  const PLAIN = 'fetchbox --data-dir ./data reset-password';
</script>

<Modal {open} title={t('forgot.title')} {onclose} size="lg">
  <div class="grid gap-4" data-part="forgot-password">
    {#if auth.canChangePassword}
      <p>{t('forgot.intro')}</p>

      <section class="grid gap-2">
        <h3 class="font-semibold">{t('forgot.docker')}</h3>
        <p class="hint">{t('forgot.dockerSteps')}</p>
        {#each DOCKER as command (command)}
          <CommandBlock {command} />
        {/each}
      </section>

      <section class="grid gap-2">
        <h3 class="font-semibold">{t('forgot.plain')}</h3>
        <p class="hint">{t('forgot.plainSteps')}</p>
        <CommandBlock command={PLAIN} />
      </section>

      <p class="hint">{t('forgot.after')}</p>
    {:else}
      <p>{t('forgot.external')}</p>
    {/if}
  </div>

  {#snippet footer()}
    <button class="btn" type="button" onclick={onclose}>{t('common.close')}</button>
  {/snippet}
</Modal>
