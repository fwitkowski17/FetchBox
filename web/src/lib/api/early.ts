// Requests the page starts itself (inline script in `index.html`) before the bundle has loaded.
// The app takes the answer instead of asking again, which saves a round trip or two on every cold
// start: the bundle and the first data download in parallel instead of one after the other.

/** An early answer older than this is not trusted any more (the data could have changed). */
const MAX_AGE_MS = 10_000;

interface EarlyRequests {
  startedAt?: number;
  [path: string]: Promise<Response> | number | undefined;
}

declare global {
  interface Window {
    __fetchboxEarly?: EarlyRequests;
  }
}

/**
 * The early response for `path` (`/downloads`, not `/api/downloads`), once. `null` when there is
 * none, it was taken already, or it is too old. The caller must still check `ok` and fall back to
 * a normal request: an early `/downloads` asked before sign-in is a `401`.
 */
export function takeEarly(path: string, now = Date.now()): Promise<Response> | null {
  const early = typeof window === 'undefined' ? undefined : window.__fetchboxEarly;
  if (!early) return null;
  const entry = early[path];
  delete early[path];
  if (!(entry instanceof Promise)) return null;
  if (now - (early.startedAt ?? 0) > MAX_AGE_MS) return null;
  return entry;
}
