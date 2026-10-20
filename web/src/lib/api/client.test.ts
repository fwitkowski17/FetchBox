import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, api, setUnauthorizedHandler } from './client';

function respond(status: number, body?: unknown): Response {
  return new Response(body === undefined ? null : JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function mockFetch(res: Response | Error) {
  const fn = vi.fn(async () => {
    if (res instanceof Error) throw res;
    return res;
  });
  vi.stubGlobal('fetch', fn);
  return fn;
}

afterEach(() => {
  vi.unstubAllGlobals();
  setUnauthorizedHandler(null);
});

describe('api client', () => {
  it('sends JSON with the session cookie and parses the answer', async () => {
    const fn = mockFetch(respond(201, { id: 7 }));
    const out = await api.addDownload({ url: 'http://x/a', on_conflict: 'rename' });
    expect(out).toEqual({ id: 7 });
    const [url, init] = fn.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/api/downloads');
    expect(init.method).toBe('POST');
    expect(init.credentials).toBe('same-origin');
    expect(JSON.parse(String(init.body))).toEqual({ url: 'http://x/a', on_conflict: 'rename' });
  });

  it('handles 204 without a body', async () => {
    mockFetch(respond(204));
    await expect(api.remove(3)).resolves.toBeUndefined();
  });

  it('exposes the conflict description of a 409', async () => {
    const conflict = {
      kind: 'file_exists',
      existing_name: 'a.bin',
      suggested_name: 'a (1).bin',
    };
    mockFetch(respond(409, { error: 'conflict', conflict }));
    const err = await api.addDownload({ url: 'http://x/a' }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).status).toBe(409);
    expect((err as ApiError).conflict).toEqual(conflict);
  });

  it('exposes the host key problem of an SSH server', async () => {
    const host_key = {
      kind: 'untrusted',
      host: 'nas.lan',
      port: 22,
      key_type: 'ssh-ed25519',
      fingerprint: 'SHA256:abc',
      trusted_fingerprint: null,
    };
    mockFetch(respond(409, { error: 'untrusted_host', host_key }));
    const err = (await api
      .probeUrl('sftp://nas.lan/a.iso', { username: 'me', password: 'pw' })
      .catch((e: unknown) => e)) as ApiError;
    expect(err.status).toBe(409);
    expect(err.hostKey).toEqual(host_key);
    expect(err.conflict).toBeUndefined();
  });

  it('sends the login with a probe and the key the user accepted', async () => {
    const fn = mockFetch(respond(200, {}));
    await api.probeUrl('ftp://nas.lan/', { username: 'me', password: 'pw' });
    const [, init] = fn.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(String(init.body))).toEqual({
      url: 'ftp://nas.lan/',
      credentials: { username: 'me', password: 'pw' },
    });
    const trust = { host: 'nas.lan', port: 22, key_type: 'ssh-ed25519', fingerprint: 'SHA256:abc' };
    await api.trustHost(trust);
    const [url, trustInit] = fn.mock.calls[1] as unknown as [string, RequestInit];
    expect(url).toBe('/api/known-hosts');
    expect(JSON.parse(String(trustInit.body))).toEqual(trust);
  });

  it('keeps the server message and retry-after of a lockout', async () => {
    mockFetch(respond(429, { error: 'too many attempts', retry_after: 42 }));
    const err = (await api.login('x').catch((e: unknown) => e)) as ApiError;
    expect(err.status).toBe(429);
    expect(err.retryAfter).toBe(42);
    expect(err.serverMessage).toBe('too many attempts');
  });

  it('reports an unreachable server as status 0', async () => {
    mockFetch(new TypeError('failed to fetch'));
    const err = (await api.listDownloads().catch((e: unknown) => e)) as ApiError;
    expect(err.status).toBe(0);
    expect(err.serverMessage).toBeUndefined();
  });

  it('survives a non-JSON error body', async () => {
    mockFetch(new Response('<html>bad gateway</html>', { status: 502 }));
    const err = (await api.listDownloads().catch((e: unknown) => e)) as ApiError;
    expect(err.status).toBe(502);
    expect(err.serverMessage).toBeUndefined(); // the UI then shows its own, translated text
    expect(err.message).toContain('502');
  });

  it('notifies on 401 from protected endpoints but not from the login itself', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    mockFetch(respond(401, { error: 'unauthorized' }));
    await api.listDownloads().catch(() => undefined);
    expect(handler).toHaveBeenCalledTimes(1);
    await api.login('wrong').catch(() => undefined);
    expect(handler).toHaveBeenCalledTimes(1);
  });

  describe('answers the page asked for early', () => {
    function early(path: string, res: Promise<Response>) {
      vi.stubGlobal('window', { __fetchboxEarly: { startedAt: Date.now(), [path]: res } });
    }

    it('uses a good early answer instead of asking again', async () => {
      const fn = mockFetch(respond(200, []));
      early('/downloads', Promise.resolve(respond(200, [{ id: 1 }])));
      expect(await api.listDownloads()).toEqual([{ id: 1 }]);
      expect(fn).not.toHaveBeenCalled();
      // Only once: the next call is a real request.
      expect(await api.listDownloads()).toEqual([]);
      expect(fn).toHaveBeenCalledTimes(1);
    });

    it('asks again when the early answer was a refusal (asked before sign-in)', async () => {
      const fn = mockFetch(respond(200, [{ id: 2 }]));
      const handler = vi.fn();
      setUnauthorizedHandler(handler);
      early('/downloads', Promise.resolve(respond(401, { error: 'unauthorized' })));
      expect(await api.listDownloads()).toEqual([{ id: 2 }]);
      expect(fn).toHaveBeenCalledTimes(1);
      expect(handler).not.toHaveBeenCalled(); // a stale 401 must not sign anybody out
    });

    it('asks again when the early request failed', async () => {
      const fn = mockFetch(respond(200, { auth_required: true }));
      const failed = Promise.reject(new TypeError('failed to fetch'));
      failed.catch(() => {});
      early('/auth/status', failed);
      await expect(api.authStatus()).resolves.toEqual({ auth_required: true });
      expect(fn).toHaveBeenCalledTimes(1);
    });

    it('never reuses an early answer for a command', async () => {
      const fn = mockFetch(respond(200, { id: 3 }));
      early('/downloads', Promise.resolve(respond(200, [])));
      await api.addDownload({ url: 'http://x/a', on_conflict: 'rename' });
      expect(fn).toHaveBeenCalledTimes(1);
    });
  });
  describe('plugins', () => {
    it('uploads a ZIP as it is, with its own content type', async () => {
      const fn = mockFetch(respond(201, { token: 't', id: 'demo' }));
      const bytes = new Uint8Array([80, 75, 3, 4]).buffer;
      await api.stagePlugin(bytes);
      const [url, init] = fn.mock.calls[0] as unknown as [string, RequestInit];
      expect(url).toBe('/api/plugin-staging');
      expect(init.method).toBe('POST');
      expect((init.headers as Record<string, string>)['content-type']).toBe('application/zip');
      expect(init.body).toBe(bytes);
    });

    it('names what to change in the settings: values store, clear empties', async () => {
      const fn = mockFetch(respond(200, { id: 'demo' }));
      await api.savePluginSettings('demo', { name: 'Filip', port: 8080, on: true }, ['token']);
      const [url, init] = fn.mock.calls[0] as unknown as [string, RequestInit];
      expect(url).toBe('/api/plugins/demo/settings');
      expect(init.method).toBe('PUT');
      expect(JSON.parse(String(init.body))).toEqual({
        values: { name: 'Filip', port: 8080, on: true },
        clear: ['token'],
      });
    });

    it('never lets an id or a token change the path', async () => {
      const fn = mockFetch(respond(204));
      await api.removePlugin('../x', true);
      await api.cancelStaged('a/b');
      await api.runPluginAction('p q', 'do/it', { m: 'x' });
      const calls = fn.mock.calls.map((c) => (c as unknown as [string, RequestInit])[0]);
      expect(calls[0]).toBe('/api/plugins/..%2Fx?keep_data=true');
      expect(calls[1]).toBe('/api/plugin-staging/a%2Fb');
      expect(calls[2]).toBe('/api/plugins/p%20q/actions/do%2Fit');
    });

    it('asks for one dependency, or for all of them', async () => {
      const fn = mockFetch(respond(200, { id: 'demo' }));
      await api.fetchPluginDependencies('demo', 'ffmpeg');
      await api.fetchPluginDependencies('demo');
      const bodies = fn.mock.calls.map((c) =>
        JSON.parse(String((c as unknown as [string, RequestInit])[1].body)),
      );
      expect(bodies).toEqual([{ dependency: 'ffmpeg' }, {}]);
    });
  });
});

