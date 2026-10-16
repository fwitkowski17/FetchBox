import { describe, expect, it } from 'vitest';
import { ALGOS, checkChecksum, detectAlgo } from './checksum';

const MD5 = 'd41d8cd98f00b204e9800998ecf8427e';
const SHA256 = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';
const SHA1 = 'da39a3ee5e6b4b0d3255bfef95601890afd80709';
const SHA512 = 'cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e';

describe('detectAlgo', () => {
  it('tells the algorithm by the length of the digest', () => {
    expect(detectAlgo(MD5)).toBe('md5');
    expect(detectAlgo(SHA1)).toBe('sha1');
    expect(detectAlgo(SHA256)).toBe('sha256');
    expect(detectAlgo(SHA512)).toBe('sha512');
    expect(ALGOS.map((a) => a.length)).toEqual([32, 40, 64, 128]);
  });

  it('prefers a label in the pasted text, and understands checksum-file lines', () => {
    expect(detectAlgo(`SHA256: ${SHA256}`)).toBe('sha256');
    expect(detectAlgo(`md5sum = ${MD5}`)).toBe('md5');
    expect(detectAlgo(`${SHA256}  ubuntu.iso`)).toBe('sha256');
    expect(detectAlgo(`${SHA1} *file.bin`)).toBe('sha1');
  });

  it('says nothing about what is not a supported digest', () => {
    expect(detectAlgo('')).toBeNull();
    expect(detectAlgo('hello world')).toBeNull();
    expect(detectAlgo('abc123')).toBeNull();
    expect(detectAlgo('z'.repeat(64))).toBeNull();
  });
});

describe('checkChecksum', () => {
  it('normalises what people copy', () => {
    expect(checkChecksum('sha256', SHA256.toUpperCase())).toEqual({ ok: true, value: SHA256 });
    expect(checkChecksum('sha256', `  sha256:${SHA256}\n`)).toEqual({ ok: true, value: SHA256 });
    expect(checkChecksum('sha256', `0x${SHA256}`)).toEqual({ ok: true, value: SHA256 });
    expect(checkChecksum('sha256', `${SHA256}  ubuntu-24.04.iso`)).toEqual({ ok: true, value: SHA256 });
    expect(checkChecksum('sha256', `${SHA256}\nsecond line`)).toEqual({ ok: true, value: SHA256 });
    const colons = MD5.match(/../g)?.join(':') ?? '';
    expect(checkChecksum('md5', colons)).toEqual({ ok: true, value: MD5 });
    const groups = MD5.match(/.{8}/g)?.join(' ') ?? '';
    expect(checkChecksum('md5', groups)).toEqual({ ok: true, value: MD5 });
  });

  it('says what is wrong', () => {
    expect(checkChecksum('md5', '  ')).toEqual({ ok: false, reason: 'empty' });
    expect(checkChecksum('md5', 'z'.repeat(32))).toEqual({ ok: false, reason: 'chars' });
    expect(checkChecksum('sha256', MD5)).toEqual({ ok: false, reason: 'length', expected: 64, got: 32 });
  });
});
