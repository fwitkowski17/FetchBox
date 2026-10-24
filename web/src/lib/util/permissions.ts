// What a plugin asks for, as groups the install dialog can show as accordions, and what an update
// adds to it. The summary is worked out by the server from the manifest; this only arranges it.
import type { PermissionSummary } from '../api/types';
import type { IconName } from '../components/Icon.svelte';

export type GroupId = PermissionSummary['groups'][number]['id'];

/** The order the groups are shown in (the server sends them in this order too). */
export const GROUP_ORDER: readonly GroupId[] = [
  'network',
  'programs',
  'downloads',
  'background',
  'events',
  'api',
];

export const GROUP_ICONS: Record<GroupId, IconName> = {
  network: 'server',
  programs: 'program',
  downloads: 'folder',
  background: 'layers',
  events: 'activity',
  api: 'key',
};

/** The lists of the summary that belong to one group, and the flag that stands for the others. */
type ListKind = 'programs' | 'download_hosts' | 'network' | 'webhook_hosts' | 'events' | 'api';
const GROUP_LISTS: Record<GroupId, readonly ListKind[]> = {
  network: ['network', 'download_hosts', 'webhook_hosts'],
  programs: ['programs'],
  downloads: [],
  background: [],
  events: ['events'],
  api: ['api'],
};

export interface PermissionRow {
  kind: ListKind;
  items: string[];
  /** The items an update adds to what the installed version had (all of them when new). */
  added: string[];
}

export interface PermissionGroup {
  id: GroupId;
  /** The manifest lists the group in `permissions.uses` (a group with content that it does not list is still shown). */
  declared: boolean;
  /** The author's reason in English, and the translations the manifest brings. */
  reason: string | null;
  reasonI18n: Record<string, string>;
  rows: PermissionRow[];
  /** The group has no list: it is a yes (a running program, downloads of its own). */
  flag: boolean;
  /** How many items the lists hold together. */
  count: number;
  /** An update asks for this group, or for more in it, than the installed version did. */
  added: boolean;
}

export interface PermissionView {
  groups: PermissionGroup[];
  /** Nothing is asked for at all. */
  none: boolean;
  /** An update asks for more than the installed version did. */
  widens: boolean;
}

function flagOn(s: PermissionSummary, id: GroupId): boolean {
  if (id === 'background') return s.process;
  if (id === 'downloads') return s.runs_downloads;
  return false;
}

function hasContent(s: PermissionSummary, id: GroupId): boolean {
  return flagOn(s, id) || GROUP_LISTS[id].some((kind) => s[kind].length > 0);
}

/** Arrange `next` for display; `previous` (the installed version, on an update) marks what is new. */
export function permissionView(
  next: PermissionSummary,
  previous?: PermissionSummary | null,
): PermissionView {
  const declared = new Map(next.groups.map((g) => [g.id, g]));
  const before = new Set(previous ? previous.groups.map((g) => g.id) : []);
  const groups: PermissionGroup[] = [];
  for (const id of GROUP_ORDER) {
    const decl = declared.get(id);
    // A group with content is shown whether or not the manifest declared it: the list is what the
    // plugin can do, not what it says.
    if (!decl && !hasContent(next, id)) continue;
    const rows: PermissionRow[] = [];
    for (const kind of GROUP_LISTS[id]) {
      const items = next[kind];
      if (items.length === 0) continue;
      const known = new Set(previous ? previous[kind] : []);
      rows.push({ kind, items, added: previous ? items.filter((i) => !known.has(i)) : [...items] });
    }
    const flag = flagOn(next, id);
    const added =
      previous != null &&
      (!before.has(id) ||
        rows.some((r) => r.added.length > 0) ||
        (flag && !flagOn(previous, id)));
    groups.push({
      id,
      declared: decl !== undefined,
      reason: decl?.reason ?? null,
      reasonI18n: decl?.reason_i18n ?? {},
      rows,
      flag,
      count: rows.reduce((n, r) => n + r.items.length, 0),
      added: previous ? added : false,
    });
  }
  return {
    groups,
    none: groups.length === 0,
    widens: previous != null && groups.some((g) => g.added),
  };
}

/** The reason in the language of the interface (`pl-PL` tries `pl-PL`, then `pl`), else English. */
export function groupReason(group: PermissionGroup, locale: string): string | null {
  if (group.reason === null) return null;
  const bare = locale.split('-')[0] ?? locale;
  return group.reasonI18n[locale] ?? group.reasonI18n[bare] ?? group.reason;
}
