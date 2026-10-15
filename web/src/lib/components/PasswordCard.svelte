<script lang="ts">
  // The "Password" card of Settings: a button that opens the dialog, or, for a password that comes
  // from the server's environment, the explanation why there is none.
  import { t } from '../i18n';
  import { auth } from '../stores/auth.svelte';
  import { CARD } from '../util/classes';
  import ChangePasswordDialog from './ChangePasswordDialog.svelte';
  import Icon from './Icon.svelte';

  let open = $state(false);
</script>

<section class={CARD} data-part="password">
  <h2 class="font-semibold tracking-tight">{t('settings.password')}</h2>
  {#if auth.canChangePassword}
    <p class="hint">{t('settings.password.hint')}</p>
    <button class="btn justify-self-start" type="button" onclick={() => (open = true)}>
      <Icon name="lock" size={18} />{t('settings.password.button')}
    </button>
    <ChangePasswordDialog {open} onclose={() => (open = false)} />
  {:else}
    <p class="hint">{t('settings.password.external')}</p>
  {/if}
</section>
