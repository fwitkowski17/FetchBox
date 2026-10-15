<script lang="ts">
  // Shown over the whole app while the connection to the server is lost. It replaces a status
  // line: when everything works there is nothing to look at, and when it does not, nothing on the
  // page can be trusted (the list is old, commands would fail), so the page is covered until the
  // app has reconnected by itself. The dialog cannot be dismissed.
  import { t } from '../i18n';
  import { downloads } from '../stores/downloads.svelte';
  import Modal from './Modal.svelte';

  /** A reconnect takes about a second (the browser's retry), so a quick restart or a network hiccup must
   *  not flash a dialog: only an outage that lasts shows it. */
  const SHOW_AFTER_MS = 1500;

  const lost = $derived(downloads.everLive && downloads.connection !== 'live');
  let shown = $state(false);

  $effect(() => {
    if (!lost) {
      shown = false;
      return;
    }
    const timer = setTimeout(() => (shown = true), SHOW_AFTER_MS);
    return () => clearTimeout(timer);
  });
</script>

<Modal open={shown} title={t('connection.lost.title')} onclose={() => {}} dismissible={false} size="sm">
  <div class="grid justify-items-center gap-3 py-2 text-center" data-part="connection-lost">
    <svg class="spinner size-12 animate-spin text-accent" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="12" cy="12" r="9" stroke="currentColor" stroke-opacity="0.2" stroke-width="2.5" />
      <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" />
    </svg>
    <p role="status">{t('connection.lost.text')}</p>
    <p class="hint">{t('connection.lost.hint')}</p>
  </div>
</Modal>
