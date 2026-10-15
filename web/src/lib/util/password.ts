// Client-side checks for new passwords. The server enforces the same bounds; this only saves a
// round trip and gives a specific message.

/** Mirrors the server's `MIN_PASSWORD_LEN`. */
export const MIN_PASSWORD_LEN = 8;
/** Mirrors the server's upper bound. */
export const MAX_PASSWORD_LEN = 256;

export type PasswordProblem = 'short' | 'long' | 'mismatch';

/** What is wrong with a new password and its repetition, if anything. Length counts characters,
 *  like the server (so an emoji is one). */
export function checkNewPassword(password: string, repeat: string): PasswordProblem | null {
  const n = [...password].length;
  if (n < MIN_PASSWORD_LEN) return 'short';
  if (n > MAX_PASSWORD_LEN) return 'long';
  if (password !== repeat) return 'mismatch';
  return null;
}
