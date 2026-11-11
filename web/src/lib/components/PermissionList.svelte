<script lang="ts">
  // What a plugin asks for, grouped like the permissions of a phone: one accordion per group, closed
  // unless an update adds to it. `previous` (the installed version, on an update) marks what is new.
  import type { PermissionSummary } from '../api/types';
  import { i18n, t } from '../i18n';
  import { GROUP_ICONS, groupReason, permissionView } from '../util/permissions';
  import Icon from './Icon.svelte';

  interface Props {
    permissions: PermissionSummary;
    previous?: PermissionSummary | null;
  }

  let { permissions, previous = null }: Props = $props();

  const view = $derived(permissionView(permissions, previous));
  // What the user opened or closed; a group nobody touched is open when an update widened it.
  let toggled = $state<Record<string, boolean>>({});
  const uid = $props.id();

  const isOpen = (id: string, added: boolean) => toggled[id] ?? added;
</script>

<div class="grid gap-2" data-part="permissions">
  {#if view.none}
    <p class="hint">{t('ext.perm.none')}</p>
  {/if}
  {#each view.groups as group (group.id)}
    {@const open = isOpen(group.id, group.added)}
    {@const reason = groupReason(group, i18n.locale)}
    <section class="overflow-hidden rounded-lg border border-border" data-group={group.id}>
      <h4>
        <button
          type="button"
          class="flex w-full items-center gap-3 px-3 py-2.5 text-left"
          aria-expanded={open}
          aria-controls={open ? `${uid}-${group.id}` : undefined}
          onclick={() => (toggled[group.id] = !open)}
        >
          <span class="grid size-9 shrink-0 place-items-center rounded-full bg-accent-soft text-accent">
            <Icon name={GROUP_ICONS[group.id]} size={18} />
          </span>
          <span class="min-w-0 flex-1">
            <span class="block text-sm font-semibold">
              {t(`ext.perm.group.${group.id}`)}
              {#if group.added}<span class="font-normal text-warning"> · {t('ext.perm.new')}</span>{/if}
            </span>
            <span class="hint block">
              {#if group.count > 0 && group.id !== 'background' && group.id !== 'downloads'}
                {t(`ext.perm.count.${group.id}`, { n: group.count })}
              {:else}
                {t(`ext.perm.group.${group.id}.about`)}
              {/if}
            </span>
          </span>
          <span class={['shrink-0 text-muted transition-transform', open && 'rotate-90']}>
            <Icon name="chevronRight" size={18} />
          </span>
        </button>
      </h4>
      {#if open}
        <div id="{uid}-{group.id}" class="grid gap-3 border-t border-border px-3 py-3">
          <p class="text-sm">{t(`ext.perm.group.${group.id}.about`)}</p>
          {#if reason}
            <div>
              <p class="text-xs font-semibold text-muted">{t('ext.perm.reason')}</p>
              <p class="text-sm" data-part="permission-reason">{reason}</p>
            </div>
          {:else if !group.declared}
            <p class="text-sm text-warning" role="note">{t('ext.perm.group.undeclared')}</p>
          {/if}
          {#each group.rows as row (row.kind)}
            <div>
              <p class="text-xs font-semibold text-muted">{t(`ext.perm.${row.kind}`)}</p>
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
        </div>
      {/if}
    </section>
  {/each}
</div>
