import { describe, expect, it } from 'vitest';
import type { Download } from '../api/types';
import { bulkInfo, pickSelected } from './bulk';

function dl(id: number, over: Partial<Download> = {}): Download {
  return {
    id,
    url: `http://host/${id}`,
    filename: `f${id}.bin`,
    category: 'other',
    status: 'completed',
    downloaded: 0,
    source_kind: 'http',
    resumable: true,
    subdir: '',
    ...over,
  };
}

describe('bulkInfo', () => {
  it('counts what each bulk action can take', () => {
    const info = bulkInfo([
      dl(1),
      dl(2, { status: 'failed' }),
      dl(3, { status: 'cancelled' }),
      dl(4, { status: 'downloading' }),
      dl(5, { status: 'paused' }),
      dl(6, { status: 'queued' }),
      dl(7, { archived: true }),
    ]);
    expect(info).toEqual({ total: 7, archivable: 3, archived: 1, unfinished: 3, withFile: 2 });
  });

  it('is all zero for nothing', () => {
    expect(bulkInfo([])).toEqual({ total: 0, archivable: 0, archived: 0, unfinished: 0, withFile: 0 });
  });
});

describe('pickSelected', () => {
  it('keeps list order and ignores ids that are gone', () => {
    const items = [dl(3), dl(2), dl(1)];
    expect(pickSelected(items, new Set([1, 3, 99])).map((d) => d.id)).toEqual([3, 1]);
  });
});
