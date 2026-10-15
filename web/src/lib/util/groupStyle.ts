// Icon and colour of each status group, shared by the side panel and the applied-filter tags.
import type { IconName } from '../components/Icon.svelte';
import type { Group } from './downloads';

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
