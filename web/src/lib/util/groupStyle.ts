// Icon and colour of each status group, shared by the side panel and the applied-filter tags.
import type { IconName } from '../components/Icon.svelte';
import type { Group } from './downloads';
import type { TorrentGroup } from './torrents';

export const GROUP_ICON: Record<Group, IconName> = {
  all: 'layers',
  active: 'activity',
  paused: 'pauseCircle',
  completed: 'checkCircle',
  stopped: 'xCircle',
  archived: 'archive',
};

/** Full class names, so Tailwind can see them. */
export const GROUP_COLOR: Record<Group, string> = {
  all: 'text-muted',
  active: 'text-accent',
  paused: 'text-warning',
  completed: 'text-success',
  stopped: 'text-danger',
  archived: 'text-muted',
};

export const TORRENT_GROUP_ICON: Record<TorrentGroup, IconName> = {
  all: 'layers',
  active: 'activity',
  seeding: 'up',
  paused: 'pauseCircle',
  finished: 'checkCircle',
  error: 'xCircle',
};

export const TORRENT_GROUP_COLOR: Record<TorrentGroup, string> = {
  all: 'text-muted',
  active: 'text-accent',
  seeding: 'text-success',
  paused: 'text-warning',
  finished: 'text-success',
  error: 'text-danger',
};
