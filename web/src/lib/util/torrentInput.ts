// What the "add torrent" dialog accepts.

/** Largest `.torrent` file the server takes (real ones are well below 1 MiB). */
export const MAX_TORRENT_FILE = 8 * 1024 * 1024;

/** Is this text a magnet link? Scheme in any case, like the server reads it. */
export function isMagnet(text: string): boolean {
  return /^magnet:\?/i.test(text.trimStart());
}

/** The links in a text field, one per line (or separated by spaces): `valid` are magnet links
 *  (repeats left out), `invalid` is whatever else was written. */
export function magnetsIn(text: string): { valid: string[]; invalid: string[] } {
  const valid: string[] = [];
  const invalid: string[] = [];
  for (const part of text.split(/\s+/)) {
    if (part === '') continue;
    if (!isMagnet(part)) invalid.push(part);
    else if (!valid.includes(part)) valid.push(part);
  }
  return { valid, invalid };
}
