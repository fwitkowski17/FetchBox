import { describe, expect, it } from 'vitest';
import { MAX_PASSWORD_LEN, MIN_PASSWORD_LEN, checkNewPassword } from './password';

describe('checkNewPassword', () => {
  it('accepts a long enough matching password', () => {
    expect(checkNewPassword('correct-horse', 'correct-horse')).toBeNull();
  });

  it('enforces the length bounds in characters', () => {
    expect(checkNewPassword('1234567', '1234567')).toBe('short');
    expect(checkNewPassword('a'.repeat(MIN_PASSWORD_LEN), 'a'.repeat(MIN_PASSWORD_LEN))).toBeNull();
    // Eight emoji are 16 UTF-16 units but eight characters, like on the server.
    expect(checkNewPassword('😀'.repeat(8), '😀'.repeat(8))).toBeNull();
    expect(checkNewPassword('😀'.repeat(7), '😀'.repeat(7))).toBe('short');
    const max = 'a'.repeat(MAX_PASSWORD_LEN);
    expect(checkNewPassword(max, max)).toBeNull();
    expect(checkNewPassword(max + 'a', max + 'a')).toBe('long');
  });

  it('requires both entries to match', () => {
    expect(checkNewPassword('correct-horse', 'correct-horsE')).toBe('mismatch');
    expect(checkNewPassword('correct-horse', '')).toBe('mismatch');
  });

  it('reports the length before the mismatch', () => {
    expect(checkNewPassword('short', 'other')).toBe('short');
  });
});
