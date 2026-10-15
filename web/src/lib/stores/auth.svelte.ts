// Is the user allowed in? Decides between the login screen and the app.
import { api } from '../api/client';
import { retryDelay } from '../util/retry';

/** How often the login screen checks that the server is still there. */
const PROBE_MS = 3000;

export type AuthState =
  | 'loading' // asking the server
  | 'login' // password required
  | 'ready' // in
  | 'offline'; // server unreachable

class AuthStore {
  state = $state<AuthState>('loading');
  /** A password is configured on the server (otherwise there is nothing to log out of). */
  required = $state(false);
  /** The password was created in the UI, so it can be changed there (not `FETCHBOX_PASSWORD`). */
  canChangePassword = $state(false);
  /**
   * The first-run wizard is on screen. It starts when the server reports that no account exists
   * and stays until the user finishes it, also after the account step signed them in.
   */
  wizard = $state(false);

  /**
   * The server does not answer while the login screen is up. The screen stays (what was typed is
   * kept) and the app covers it with the connection dialog: there is no point in asking for a
   * password nobody can check.
   */
  unreachable = $state(false);

  #retry: ReturnType<typeof setTimeout> | null = null;
  #attempt = 0;
  #listening = false;

  /** (Re)check with the server. Safe to call at any time. */
  async check(): Promise<void> {
    this.#cancelTimer();
    this.#listen();
    try {
      const s = await api.authStatus();
      this.#attempt = 0;
      this.unreachable = false;
      this.required = s.auth_required;
      this.canChangePassword = s.can_change_password;
      if (s.setup_required) this.wizard = true;
      this.state = s.authenticated ? 'ready' : 'login';
      this.#watchWhileLoggedOut();
    } catch {
      // The status endpoint is public: our server always answers it. Whatever came back instead is
      // not "please sign in" but "nobody is home": no network, or a proxy in front of a dead server
      // (502/503/504 from a reverse proxy, 500 from the Vite dev proxy). Keep the screen that is up
      // (the connection dialog covers it, a signed-in session survives a blip); before any screen
      // exists there is only the placeholder.
      if (this.state === 'loading' || this.state === 'offline') this.state = 'offline';
      else this.unreachable = true;
      this.#timer(retryDelay(this.#attempt++));
    }
  }

  // An unreachable server is tried again by itself (it may just be restarting, or the phone woke
  // up before the network did).
  #timer(ms: number): void {
    this.#cancelTimer();
    this.#retry = setTimeout(() => {
      // Not worth a request while nobody looks; coming back to the tab checks at once.
      if (typeof document !== 'undefined' && document.hidden && !this.unreachable) this.#timer(ms);
      else void this.check();
    }, ms);
  }

  #cancelTimer(): void {
    if (this.#retry) clearTimeout(this.#retry);
    this.#retry = null;
  }

  /** On the login screen nothing else talks to the server, so the app would not notice it dying:
   *  keep asking, quietly. */
  #watchWhileLoggedOut(): void {
    if (this.state === 'login') this.#timer(PROBE_MS);
  }

  #listen(): void {
    if (this.#listening || typeof window === 'undefined') return;
    this.#listening = true;
    const again = (): void => {
      if (this.state === 'offline' || this.state === 'login') void this.check();
    };
    window.addEventListener('online', again);
    document.addEventListener('visibilitychange', () => {
      if (!document.hidden) again();
    });
  }

  /** Throws `ApiError` (401 wrong password, 429 locked out). */
  async login(password: string): Promise<void> {
    await api.login(password);
    this.#cancelTimer();
    this.unreachable = false;
    this.state = 'ready';
  }

  /**
   * First run: create the account with the code from the server log. Signs in on success.
   * Throws `ApiError` (403 wrong code, 400 weak password, 409 already set up, 429 locked out).
   */
  async setup(code: string, password: string): Promise<void> {
    await api.setup(code, password);
    this.#cancelTimer();
    this.unreachable = false;
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
    this.#watchWhileLoggedOut();
  }

  /** A protected call answered 401: the session ended. */
  expire(): void {
    if (this.state !== 'ready') return;
    this.state = 'login';
    this.#watchWhileLoggedOut();
  }
}

export const auth = new AuthStore();
