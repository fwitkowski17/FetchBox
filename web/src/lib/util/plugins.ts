// Plugins in the interface (SPEC 6.15): applying live events to the list, and the small questions
// the screens ask about a plugin. Pure, so it can be tested without a browser.
import type { AppEvent, NoticeLevel, PluginState, PluginView } from '../api/types';

/** Apply one live event to the list of plugins. Returns the same array when nothing changed. */
export function applyPluginEvent(items: PluginView[], ev: AppEvent): PluginView[] {
  switch (ev.type) {
    case 'plugin': {
      const i = items.findIndex((p) => p.id === ev.plugin.id);
      if (i === -1) return sortPlugins([...items, ev.plugin]);
      const next = items.slice();
      next[i] = ev.plugin;
      return next;
    }
    case 'plugin_removed':
      return items.some((p) => p.id === ev.id) ? items.filter((p) => p.id !== ev.id) : items;
    default:
      return items;
  }
}

/** By name, so a card does not jump when its state changes. */
export function sortPlugins(items: readonly PluginView[]): PluginView[] {
  return items.slice().sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

/** The plugin is doing something or waits for the user: it needs a look. */
export function needsAttention(p: PluginView): boolean {
  return p.state === 'needs_review' || p.state === 'failed' || p.update_available != null;
}

/** How many plugins want attention (the dot in the side panel). */
export function attentionCount(items: readonly PluginView[]): number {
  return items.filter(needsAttention).length;
}

/** The plugin can run and be used now. */
export function isUsable(p: PluginView): boolean {
  return p.state === 'ready' || p.state === 'running';
}

/** Why a plugin cannot be switched on, if it cannot; `null` when it can. */
export function cannotEnable(p: PluginView): PluginState | null {
  return p.state === 'needs_review' || p.state === 'unsupported' ? p.state : null;
}

export type Tone = 'good' | 'busy' | 'bad' | 'muted' | 'warn';

/** The colour family of a plugin's state badge. */
export function stateTone(state: PluginState): Tone {
  switch (state) {
    case 'running':
    case 'ready':
      return 'good';
    case 'preparing':
    case 'starting':
      return 'busy';
    case 'failed':
      return 'bad';
    case 'needs_review':
    case 'unsupported':
      return 'warn';
    case 'disabled':
      return 'muted';
  }
}

/** The tone of a toast for a message from a plugin. */
export function noticeTone(level: NoticeLevel): 'info' | 'success' | 'warning' | 'error' {
  return level;
}

/** Percent of the dependency that has arrived, or `null` when the size is not known. */
export function dependencyPercent(p: PluginView): number | null {
  const d = p.progress;
  if (!d || !d.total) return null;
  return Math.min(100, Math.floor((d.downloaded / d.total) * 100));
}
