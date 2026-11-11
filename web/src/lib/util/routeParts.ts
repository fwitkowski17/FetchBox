/** A hash such as `#/settings/extensions` split into the page and what follows it. */
export interface HashParts {
  /** First segment, with a leading slash (`/settings`); `/` for an empty hash. */
  page: string;
  /** The segments after the page (`['extensions']`), decoded. */
  tail: string[];
}

export function splitHash(hash: string): HashParts {
  const path = hash.replace(/^#/, '').split('?')[0] ?? '';
  const segments = path.split('/').filter((s) => s !== '');
  const [first, ...rest] = segments;
  return {
    page: first === undefined ? '/' : `/${first}`,
    tail: rest.map((s) => {
      try {
        return decodeURIComponent(s);
      } catch {
        return s;
      }
    }),
  };
}
