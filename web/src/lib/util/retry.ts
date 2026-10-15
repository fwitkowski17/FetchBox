// How long to wait before trying to reach the server again: quick at first (a restart takes a
// moment), then calmer so a server that is really gone is not hammered.

const STEPS_MS = [500, 1000, 2000, 4000];
const MAX_MS = 5000;

/** Delay before retry number `attempt` (0 = the first retry). */
export function retryDelay(attempt: number): number {
  const i = Math.max(0, Math.floor(attempt));
  return i < STEPS_MS.length ? (STEPS_MS[i] ?? MAX_MS) : MAX_MS;
}
