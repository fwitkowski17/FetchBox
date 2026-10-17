import { readable } from 'svelte/store';

export type Route = 'downloads' | 'torrents' | 'stats' | 'settings';

/** Labels are translated: `nav.<id>`. */
export const routes: readonly { id: Route; path: string }[] = [
  { id: 'downloads', path: '/' },
  { id: 'torrents', path: '/torrents' },
  { id: 'stats', path: '/stats' },
  { id: 'settings', path: '/settings' },
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
