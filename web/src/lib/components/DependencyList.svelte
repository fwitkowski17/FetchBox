<script lang="ts">
  // The programs an extension needs besides its own files, shown before it is installed: where each
  // comes from, how big it is, and whether it will be downloaded. Not permissions.
  import { i18n, t } from '../i18n';
  import type { DependencyReview } from '../util/dependencies';
  import { formatBytes } from '../util/format';
  import Icon from './Icon.svelte';

  interface Props {
    review: DependencyReview;
  }

  let { review }: Props = $props();

  const size = (bytes: number | null | undefined): string =>
    bytes == null
      ? t('ext.install.depsSizeUnknown')
      : t('ext.install.depsSize', { size: formatBytes(bytes, i18n.locale) });
</script>

<ul class="grid gap-2" data-part="dependencies">
  {#each review.rows as row (row.need.id)}
    <li
      class="grid gap-1 rounded-lg border border-border px-3 py-2.5"
      data-dependency={row.need.id}
      data-plan={row.plan}
    >
      <div class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
        <span class="text-sm font-semibold">{row.need.label}</span>
        <code class="rounded-md bg-surface-2 px-1.5 py-0.5 font-mono text-xs">{row.need.id}</code>
        <span class="text-xs text-muted">{row.need.optional ? t('ext.dep.optional') : t('ext.dep.required')}</span>
        {#if row.added}<span class="text-xs text-warning">· {t('ext.dep.new')}</span>{/if}
      </div>
      <p class="hint [overflow-wrap:anywhere]">
        {[t(`ext.dep.source.${row.need.source}`), ...row.need.hosts].join(' · ')}
      </p>
      <p class={['flex items-center gap-1.5 text-sm', row.plan === 'present' && 'text-success']}>
        {#if row.plan === 'present'}
          <Icon name="check" size={16} />
        {:else if row.plan === 'download'}
          <Icon name="down" size={16} />
        {/if}
        <span>{t(`ext.dep.plan.${row.plan}`)}</span>
        {#if row.plan !== 'present'}<span class="text-muted">· {size(row.need.size)}</span>{/if}
      </p>
    </li>
  {/each}
</ul>
