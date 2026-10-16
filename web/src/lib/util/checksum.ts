// Checksums as people paste them (SPEC 6.2.2). The server has the last word: this only helps while
// typing (recognising the type, saying what is wrong), and follows the same rules as the server.
import type { ChecksumAlgo } from '../api/types';

export interface AlgoInfo {
  id: ChecksumAlgo;
  /** As sites write it. */
  label: string;
  /** Characters of the digest in hexadecimal. */
  length: number;
}

/** Shortest first. The length alone tells which one a pasted digest is. */
export const ALGOS: readonly AlgoInfo[] = [
  { id: 'md5', label: 'MD5', length: 32 },
  { id: 'sha1', label: 'SHA-1', length: 40 },
  { id: 'sha256', label: 'SHA-256', length: 64 },
  { id: 'sha512', label: 'SHA-512', length: 128 },
];

export function algoInfo(id: ChecksumAlgo): AlgoInfo {
  return ALGOS.find((a) => a.id === id) ?? ALGOS[2]!;
}

export function algoLabel(id: ChecksumAlgo): string {
  return algoInfo(id).label;
}

/** `sha256:abc…`, `SHA256 = abc…`, `sha256sum: abc…` */
const PREFIX = /^(md5|sha1|sha256|sha512)(?:sum)?\s*[:=]\s*/i;

/** What is left of a pasted line once the label, `0x` and the file name are gone, and the
 *  algorithm the label named, if any. */
function cleaned(pasted: string): { text: string; named: ChecksumAlgo | null } {
  let text = (pasted.trim().split(/\r?\n/)[0] ?? '').trim().toLowerCase();
  let named: ChecksumAlgo | null = null;
  const m = PREFIX.exec(text);
  if (m) {
    named = m[1] as ChecksumAlgo;
    text = text.slice(m[0].length).trim();
  }
  if (text.startsWith('0x')) text = text.slice(2);
  return { text, named };
}

/** The digest in a pasted text: the first word of a checksum-file line when that has the right
 *  length for `length`, otherwise the whole text without separators (`d4:1d:...`, groups of eight). */
function digestOf(text: string, length: number | null): string {
  const first = text.split(/\s+/)[0] ?? '';
  if (length !== null && first.length === length) return first;
  const joined = text.replace(/[:\s-]/g, '');
  if (length === null && /^[0-9a-f]+$/.test(first) && ALGOS.some((a) => a.length === first.length)) return first;
  return joined;
}

/** The algorithm a pasted checksum must be, by its label or else by its length; `null` when it
 *  cannot be told (not a digest, or a length no supported algorithm has). */
export function detectAlgo(pasted: string): ChecksumAlgo | null {
  const { text, named } = cleaned(pasted);
  if (named) return named;
  const digest = digestOf(text, null);
  if (!/^[0-9a-f]+$/.test(digest)) return null;
  return ALGOS.find((a) => a.length === digest.length)?.id ?? null;
}

export type ChecksumCheck =
  | { ok: true; value: string }
  | { ok: false; reason: 'empty' }
  | { ok: false; reason: 'chars' }
  | { ok: false; reason: 'length'; expected: number; got: number };

/** The normal form of `pasted` for `algo`, or what is wrong with it. */
export function checkChecksum(algo: ChecksumAlgo, pasted: string): ChecksumCheck {
  const { text } = cleaned(pasted);
  if (text === '') return { ok: false, reason: 'empty' };
  const length = algoInfo(algo).length;
  const digest = digestOf(text, length);
  if (!/^[0-9a-f]*$/.test(digest)) return { ok: false, reason: 'chars' };
  if (digest.length !== length) return { ok: false, reason: 'length', expected: length, got: digest.length };
  return { ok: true, value: digest };
}
