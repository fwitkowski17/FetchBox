import { describe, expect, it } from 'vitest';
import type { Download } from '../api/types';
import { bulkInfo, hasFinishedFile, pickSelected } from './bulk';

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

describe('downloads with a checksum', () => {
  it('have a finished file while verifying and after a failed verification', () => {
    expect(hasFinishedFile(dl(1))).toBe(true);
    expect(hasFinishedFile(dl(2, { status: 'verifying' }))).toBe(true);
    expect(hasFinishedFile(dl(3, { status: 'failed', verify_failed: true }))).toBe(true);
    // A plain failure left only a partial file.
    expect(hasFinishedFile(dl(4, { status: 'failed' }))).toBe(false);
    expect(hasFinishedFile(dl(5, { status: 'downloading' }))).toBe(false);
  });

  it('offer deleting the file, and are not "unfinished" while only the checking is stopped', () => {
    const info = bulkInfo([dl(1, { status: 'verifying' }), dl(2, { status: 'failed', verify_failed: true })]);
    expect(info.withFile).toBe(2);
    expect(info.unfinished).toBe(0);
    expect(info.archivable).toBe(1); // only the failed one: a verification in progress is not over
  });
});

describe('pickSelected', () => {
  it('keeps list order and ignores ids that are gone', () => {
    const items = [dl(3), dl(2), dl(1)];
    expect(pickSelected(items, new Set([1, 3, 99])).map((d) => d.id)).toEqual([3, 1]);
  });
});
