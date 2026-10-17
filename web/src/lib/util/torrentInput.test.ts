import { describe, expect, it } from 'vitest';
import { isMagnet, magnetsIn } from './torrentInput';

describe('isMagnet', () => {
  it('reads the scheme in any case and ignores space in front', () => {
    expect(isMagnet('magnet:?xt=urn:btih:abc')).toBe(true);
    expect(isMagnet('  MAGNET:?xt=urn:btih:abc')).toBe(true);
    expect(isMagnet('magnet:')).toBe(false);
    expect(isMagnet('https://example.com/a.torrent')).toBe(false);
    expect(isMagnet('')).toBe(false);
  });
});

describe('magnetsIn', () => {
  it('takes one link per line and drops repeats and blank lines', () => {
    const a = 'magnet:?xt=urn:btih:aaaa';
    const b = 'magnet:?xt=urn:btih:bbbb&dn=Name';
    expect(magnetsIn(`${a}\n\n  ${b}  \r\n${a}\n`)).toEqual({ valid: [a, b], invalid: [] });
  });
  it('reports what is not a magnet link', () => {
    expect(magnetsIn('magnet:?xt=urn:btih:aaaa\nhello')).toEqual({
      valid: ['magnet:?xt=urn:btih:aaaa'],
      invalid: ['hello'],
    });
    expect(magnetsIn('   ')).toEqual({ valid: [], invalid: [] });
  });
});
