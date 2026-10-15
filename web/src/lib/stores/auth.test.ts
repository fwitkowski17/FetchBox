import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const authStatus = vi.fn();
const login = vi.fn();

vi.mock('../api/client', async () => {
  const actual = await vi.importActual<typeof import('../api/client')>('../api/client');
  return { ...actual, api: { authStatus, login, logout: vi.fn(async () => {}) } };
});

const { ApiError } = await import('../api/client');
const { auth } = await import('./auth.svelte');

const status = (over: Record<string, unknown> = {}) => ({
  auth_required: true,
  authenticated: false,
  setup_required: false,
  can_change_password: true,
  ...over,
});
const down = () => new ApiError(0, undefined);

beforeEach(async () => {
  vi.useFakeTimers();
  authStatus.mockReset();
  login.mockReset();
  // The store is a singleton: one good answer resets what an earlier test left behind (the
  // backoff counter, a pending probe).
  authStatus.mockResolvedValueOnce(status());
  await auth.check();
  vi.clearAllTimers();
  authStatus.mockReset();
  auth.state = 'loading';
  auth.unreachable = false;
  auth.wizard = false;
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

describe('auth store and a server that is not there', () => {
  it('shows the login screen while the server is up', async () => {
    authStatus.mockResolvedValue(status());
    await auth.check();
    expect(auth.state).toBe('login');
    expect(auth.unreachable).toBe(false);
  });

  it('keeps the login screen when the server dies, and says so', async () => {
    authStatus.mockResolvedValueOnce(status());
    await auth.check();
    // The next quiet check finds nobody home.
    authStatus.mockRejectedValue(down());
    await vi.advanceTimersByTimeAsync(3000);
    expect(auth.state).toBe('login'); // the screen (and a half typed password) stays
    expect(auth.unreachable).toBe(true);
  });

  it('forgets about it as soon as the server answers again', async () => {
    authStatus.mockResolvedValueOnce(status());
    await auth.check();
    authStatus.mockRejectedValueOnce(down());
    await vi.advanceTimersByTimeAsync(3000);
    expect(auth.unreachable).toBe(true);
    authStatus.mockResolvedValue(status());
    await vi.advanceTimersByTimeAsync(600); // first retry after 0.5 s
    expect(auth.unreachable).toBe(false);
    expect(auth.state).toBe('login');
  });

  it('goes straight into the app if the server came back without asking for a password', async () => {
    authStatus.mockResolvedValueOnce(status());
    await auth.check();
    authStatus.mockResolvedValue(status({ authenticated: true, auth_required: false }));
    await vi.advanceTimersByTimeAsync(3000);
    expect(auth.state).toBe('ready');
    expect(auth.unreachable).toBe(false);
  });

  it('is offline, not on a login screen, when the server is gone from the start', async () => {
    authStatus.mockRejectedValue(down());
    await auth.check();
    expect(auth.state).toBe('offline');
    expect(auth.unreachable).toBe(false);
    authStatus.mockResolvedValue(status());
    await vi.advanceTimersByTimeAsync(600);
    expect(auth.state).toBe('login');
  });

  it('stops watching once signed in', async () => {
    authStatus.mockResolvedValue(status());
    await auth.check();
    login.mockResolvedValue({ token: 't' });
    await auth.login('correct horse');
    expect(auth.state).toBe('ready');
    authStatus.mockClear();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(authStatus).not.toHaveBeenCalled();
  });

  it('an error page from a proxy in front of a dead server is a dead server, not a login screen', async () => {
    authStatus.mockResolvedValueOnce(status());
    await auth.check();
    for (const code of [500, 502, 503, 504]) {
      authStatus.mockRejectedValueOnce(new ApiError(code, undefined));
      await vi.advanceTimersByTimeAsync(3000);
      expect(auth.unreachable, String(code)).toBe(true);
      expect(auth.state).toBe('login');
      authStatus.mockResolvedValue(status());
      await vi.advanceTimersByTimeAsync(600);
      expect(auth.unreachable).toBe(false);
    }
  });

  it('is offline, not on the login screen, when the first answer is a gateway error', async () => {
    authStatus.mockRejectedValue(new ApiError(502, undefined));
    await auth.check();
    expect(auth.state).toBe('offline');
  });

  it('keeps a signed-in app on screen when the server stops answering, and recovers', async () => {
    auth.state = 'ready';
    authStatus.mockRejectedValueOnce(new ApiError(502, undefined));
    await auth.check();
    expect(auth.state).toBe('ready'); // the dialog covers it; nothing is torn down
    expect(auth.unreachable).toBe(true);
    authStatus.mockResolvedValue(status({ authenticated: true }));
    await vi.advanceTimersByTimeAsync(600);
    expect(auth.unreachable).toBe(false);
    expect(auth.state).toBe('ready');
  });

  it('goes to the login screen when the server is back but forgot the session', async () => {
    auth.state = 'ready';
    authStatus.mockRejectedValueOnce(down());
    await auth.check();
    authStatus.mockResolvedValue(status({ authenticated: false }));
    await vi.advanceTimersByTimeAsync(600);
    expect(auth.state).toBe('login');
    expect(auth.unreachable).toBe(false);
  });
});
