import { describe, expect, it } from 'vitest';
import { splitHash } from './routeParts';

describe('splitHash', () => {
  it('treats an empty hash as the downloads page', () => {
    expect(splitHash('')).toEqual({ page: '/', tail: [] });
    expect(splitHash('#')).toEqual({ page: '/', tail: [] });
    expect(splitHash('#/')).toEqual({ page: '/', tail: [] });
  });

  it('splits the page from what follows it', () => {
    expect(splitHash('#/settings')).toEqual({ page: '/settings', tail: [] });
    expect(splitHash('#/settings/extensions')).toEqual({ page: '/settings', tail: ['extensions'] });
    expect(splitHash('#/settings/ext/yt-dlp/')).toEqual({ page: '/settings', tail: ['ext', 'yt-dlp'] });
  });

  it('ignores a query string and decodes segments', () => {
    expect(splitHash('#/extensions/store?x=1')).toEqual({ page: '/extensions', tail: ['store'] });
    expect(splitHash('#/settings/a%20b')).toEqual({ page: '/settings', tail: ['a b'] });
    expect(splitHash('#/settings/%E0%A4%A')).toEqual({ page: '/settings', tail: ['%E0%A4%A'] });
  });
});
