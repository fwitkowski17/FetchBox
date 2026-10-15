// Small calculations behind the statistics page, kept apart from the components so they can be
// tested.

const BASES = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000];

/**
 * The vertical axis of a chart of bytes: three ticks (0, half, top), where the top is a round
 * number *in the unit it is written in* (1, 2, 5, 10, 20, 50... KB, MB, GB, so the labels read
 * "0 / 500 MB / 1000 MB" or "0 / 1 GB / 2 GB" and never "1.9 GB"). Nothing to plot gets a
 * 1 MB axis rather than a degenerate one.
 */
export function niceAxis(max: number): { top: number; ticks: [number, number, number] } {
  if (!Number.isFinite(max) || max <= 0) return { top: 1024 ** 2, ticks: [0, 1024 ** 2 / 2, 1024 ** 2] };
  let unit = 1;
  while (max / unit >= 1024) unit *= 1024;
  const v = max / unit;
  const base = BASES.find((b) => b >= v) ?? 1024;
  const top = base * unit;
  return { top, ticks: [0, top / 2, top] };
}

/** Height of a bar as a share of the axis, `0..1`. A bar with something in it is never invisible. */
export function barShare(value: number, top: number, minShare = 0.012): number {
  if (!(value > 0) || !(top > 0)) return 0;
  return Math.min(1, Math.max(minShare, value / top));
}

/** Share of the finished downloads that succeeded, whole percent; `null` when none has ended yet.
 *  Cancelling is the user's choice, not a failure, so it does not count against it. */
export function successPercent(completed: number, failed: number): number | null {
  const ended = completed + failed;
  return ended > 0 ? Math.round((completed / ended) * 100) : null;
}

/** "5.0 GB" is "5 GB" on an axis: whole numbers need no decimal (any decimal separator). */
export function trimAxisZero(label: string): string {
  return label.replace(/[.,]0(?=\s|$)/, '');
}
