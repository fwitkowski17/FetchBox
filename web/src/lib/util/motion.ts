// Animations respect `prefers-reduced-motion`. CSS animations are switched off globally in
// app.css; the Svelte transitions take their duration from here.

/** `ms`, or 0 when the user asked for less motion. */
export function motion(ms: number): number {
  if (typeof matchMedia !== 'function') return ms;
  return matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : ms;
}
