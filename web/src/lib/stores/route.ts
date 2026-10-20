import { readable } from 'svelte/store';
import { splitHash } from '../util/routeParts';

export type Route = 'downloads' | 'torrents' | 'stats' | 'extensions' | 'settings';

/** Labels are translated: `nav.<id>`. */
export const routes: readonly { id: Route; path: string }[] = [
  { id: 'downloads', path: '/' },
  { id: 'torrents', path: '/torrents' },
  { id: 'stats', path: '/stats' },
  { id: 'extensions', path: '/extensions' },
  { id: 'settings', path: '/settings' },
];

function parse(hash: string): Route {
  const { page } = splitHash(hash);
  return routes.find((r) => r.path === page)?.id ?? 'downloads';
}

/** Current route, derived from `location.hash`. */
export const route = readable<Route>(parse(location.hash), (set) => {
  const onChange = () => set(parse(location.hash));
  window.addEventListener('hashchange', onChange);
  return () => window.removeEventListener('hashchange', onChange);
});

/** What follows the page in the hash: `#/settings/extensions` is `['extensions']`. */
export const routeTail = readable<string[]>(splitHash(location.hash).tail, (set) => {
  // The first subscriber may come long after this module was loaded: read the hash again.
  set(splitHash(location.hash).tail);
  const onChange = () => set(splitHash(location.hash).tail);
  window.addEventListener('hashchange', onChange);
  return () => window.removeEventListener('hashchange', onChange);
});
