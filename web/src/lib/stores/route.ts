import { readable } from 'svelte/store';

export type Route = 'downloads' | 'add' | 'settings';

export const routes: readonly { id: Route; path: string; label: string }[] = [
  { id: 'downloads', path: '/', label: 'Downloads' },
  { id: 'add', path: '/add', label: 'Add' },
  { id: 'settings', path: '/settings', label: 'Settings' },
];

function parse(hash: string): Route {
  const path = hash.replace(/^#/, '') || '/';
  return routes.find((r) => r.path === path)?.id ?? 'downloads';
}

/** Current route, derived from `location.hash`. */
export const route = readable<Route>(parse(location.hash), (set) => {
  const onChange = () => set(parse(location.hash));
  window.addEventListener('hashchange', onChange);
  return () => window.removeEventListener('hashchange', onChange);
});
