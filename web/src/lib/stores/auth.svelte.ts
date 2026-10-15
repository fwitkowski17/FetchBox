// Is the user allowed in? Decides between the login screen and the app.
import { ApiError, api } from '../api/client';
import { retryDelay } from '../util/retry';

export type AuthState =
  | 'loading' // asking the server
  | 'login' // password required
  | 'ready' // in
  | 'offline'; // server unreachable

class AuthStore {
  state = $state<AuthState>('loading');
  /** A password is configured on the server (otherwise there is nothing to log out of). */
  required = $state(false);
  /** The password was created in the UI, so it can be changed there (not `DLM_PASSWORD`). */
  canChangePassword = $state(false);
  /**
   * The first-run wizard is on screen. It starts when the server reports that no account exists
   * and stays until the user finishes it, also after the account step signed them in.
   */
  wizard = $state(false);

  #retry: ReturnType<typeof setTimeout> | null = null;
  #attempt = 0;
  #listening = false;

  /** (Re)check with the server. Safe to call at any time. */
  async check(): Promise<void> {
    this.#cancelRetry();
    try {
      const s = await api.authStatus();
      this.#attempt = 0;
      this.required = s.auth_required;
      this.canChangePassword = s.can_change_password;
      if (s.setup_required) this.wizard = true;
      this.state = s.authenticated ? 'ready' : 'login';
    } catch (e) {
      const unreachable = !(e instanceof ApiError) || e.status === 0;
      this.state = unreachable ? 'offline' : 'login';
      if (unreachable) this.#scheduleRetry();
    }
  }

  // An unreachable server is tried again by itself (it may just be restarting, or the phone woke
  // up before the network did); the button on the offline screen only skips the wait.
  #scheduleRetry(): void {
    if (typeof window !== 'undefined' && !this.#listening) {
      this.#listening = true;
      window.addEventListener('online', () => {
        if (this.state === 'offline') void this.check();
      });
    }
    this.#retry = setTimeout(() => void this.check(), retryDelay(this.#attempt++));
  }

  #cancelRetry(): void {
    if (this.#retry) clearTimeout(this.#retry);
    this.#retry = null;
  }

  /** Throws `ApiError` (401 wrong password, 429 locked out). */
  async login(password: string): Promise<void> {
    await api.login(password);
    this.state = 'ready';
  }

  /**
   * First run: create the account with the code from the server log. Signs in on success.
   * Throws `ApiError` (403 wrong code, 400 weak password, 409 already set up, 429 locked out).
   */
  async setup(code: string, password: string): Promise<void> {
    await api.setup(code, password);
    this.canChangePassword = true;
    this.state = 'ready';
  }

  /** Leave the wizard for the app. */
  finishWizard(): void {
    this.wizard = false;
  }

  /** Throws `ApiError` (403 wrong current password, 400 weak, 409 managed outside the UI). */
  async changePassword(current: string, next: string): Promise<void> {
    await api.changePassword(current, next);
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
