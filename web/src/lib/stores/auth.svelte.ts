// Is the user allowed in? Decides between the login screen and the app.
import { ApiError, api } from '../api/client';

export type AuthState =
  | 'loading' // asking the server
  | 'login' // password required
  | 'ready' // in
  | 'offline'; // server unreachable

class AuthStore {
  state = $state<AuthState>('loading');
  /** A password is configured on the server (otherwise there is nothing to log out of). */
  required = $state(false);

  /** (Re)check with the server. Safe to call at any time. */
  async check(): Promise<void> {
    try {
      const s = await api.authStatus();
      this.required = s.auth_required;
      this.state = s.authenticated ? 'ready' : 'login';
    } catch (e) {
      this.state = e instanceof ApiError && e.status !== 0 ? 'login' : 'offline';
    }
  }

  /** Throws `ApiError` (401 wrong password, 429 locked out). */
  async login(password: string): Promise<void> {
    await api.login(password);
    this.state = 'ready';
  }

  async logout(): Promise<void> {
    try {
      await api.logout();
    } catch {
      // The cookie may already be invalid; either way we are leaving.
    }
    this.state = 'login';
  }

  /** A protected call answered 401: the session ended. */
  expire(): void {
    if (this.state === 'ready') this.state = 'login';
  }
}

export const auth = new AuthStore();
