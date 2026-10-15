import { afterEach, describe, expect, it, vi } from 'vitest';
import { takeEarly } from './early';

const ok = () => new Response('[]', { status: 200 });

function page(entries: Record<string, Promise<Response>>, startedAt = Date.now()) {
  vi.stubGlobal('window', { __fetchboxEarly: { startedAt, ...entries } });
}

afterEach(() => vi.unstubAllGlobals());

describe('early requests', () => {
  it('hands out the answer once', async () => {
    page({ '/downloads': Promise.resolve(ok()) });
    const first = takeEarly('/downloads');
    expect(first).not.toBeNull();
    expect((await first)?.status).toBe(200);
    expect(takeEarly('/downloads')).toBeNull();
  });

  it('knows nothing about paths the page did not ask for', () => {
    page({ '/downloads': Promise.resolve(ok()) });
    expect(takeEarly('/disk')).toBeNull();
    expect(takeEarly('/startedAt')).toBeNull(); // not a request
  });

  it('does not trust an answer from long ago', () => {
    page({ '/downloads': Promise.resolve(ok()) }, 1_000);
    expect(takeEarly('/downloads', 1_000 + 60_000)).toBeNull();
  });

  it('is simply absent without the inline script (tests, other hosts)', () => {
    expect(takeEarly('/downloads')).toBeNull();
    vi.stubGlobal('window', {});
    expect(takeEarly('/downloads')).toBeNull();
  });
});
