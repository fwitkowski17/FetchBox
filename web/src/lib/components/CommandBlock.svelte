<script lang="ts">
  // A shell command to run, with a button that copies it.
  import { t } from '../i18n';
  import { toast } from '../stores/toast.svelte';
  import { copyText } from '../util/clipboard';
  import Icon from './Icon.svelte';

  let { command }: { command: string } = $props();

  async function copy(): Promise<void> {
    if (await copyText(command)) toast.success(t('common.copied'));
  }
</script>

<div class="flex items-center gap-2 rounded-lg border border-border bg-surface-2 py-1 pr-1 pl-3">
  <code class="min-w-0 flex-1 overflow-x-auto py-1 font-mono text-sm whitespace-nowrap" data-part="command">{command}</code>
  <button class="btn icon ghost shrink-0" type="button" onclick={copy} aria-label={t('common.copy')} title={t('common.copy')}>
    <Icon name="copy" size={18} />
  </button>
</div>
