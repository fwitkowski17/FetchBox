<script lang="ts">
  // What a plugin asks for, in words. On an update, `previous` marks what is new.
  import type { PermissionSummary } from '../api/types';
  import { t } from '../i18n';
  import { permissionView } from '../util/permissions';

  interface Props {
    permissions: PermissionSummary;
    previous?: PermissionSummary | null;
  }

  let { permissions, previous = null }: Props = $props();

  const view = $derived(permissionView(permissions, previous));
</script>

<div class="grid gap-3" data-part="permissions">
  {#if view.none}
    <p class="hint">{t('ext.perm.none')}</p>
  {/if}
  {#each view.rows as row (row.kind)}
    <div>
      <p class="text-sm font-semibold">{t(`ext.perm.${row.kind}`)}</p>
      <p class="hint">{t(`ext.perm.${row.kind}.hint`)}</p>
      <ul class="mt-1 flex flex-wrap gap-1.5">
        {#each row.items as item (item)}
          <li>
            <code
              class={[
                'inline-block rounded-md px-2 py-0.5 font-mono text-xs [overflow-wrap:anywhere]',
                row.added.includes(item) && previous ? 'bg-warning-soft text-warning' : 'bg-surface-2',
              ]}
            >{item}</code>
          </li>
        {/each}
      </ul>
    </div>
  {/each}
  {#if view.process.on}
    <p class="text-sm" data-flag="process">
      <span class="font-semibold">{t('ext.perm.process')}</span>
      {#if view.process.added}<span class="text-warning"> · {t('ext.perm.new')}</span>{/if}
      <span class="hint block">{t('ext.perm.process.hint')}</span>
    </p>
  {/if}
  {#if view.runsDownloads.on}
    <p class="text-sm" data-flag="runs-downloads">
      <span class="font-semibold">{t('ext.perm.runsDownloads')}</span>
      {#if view.runsDownloads.added}<span class="text-warning"> · {t('ext.perm.new')}</span>{/if}
      <span class="hint block">{t('ext.perm.runsDownloads.hint')}</span>
    </p>
  {/if}
</div>
