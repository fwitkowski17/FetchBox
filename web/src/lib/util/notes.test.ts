import { describe, expect, it } from 'vitest';
import { inline, parseNotes } from './notes';

describe('inline', () => {
  it('marks bold and code and drops link targets', () => {
    expect(inline('a **b** `c` [d](https://x.example/y)')).toEqual([
      { text: 'a ' },
      { text: 'b', bold: true },
      { text: ' ' },
      { text: 'c', code: true },
      { text: ' d' },
    ]);
  });
  it('keeps markup it does not know as text, never as HTML', () => {
    expect(inline('<img src=x onerror=alert(1)>')).toEqual([{ text: '<img src=x onerror=alert(1)>' }]);
  });
});

describe('parseNotes', () => {
  it('reads headings, lists and paragraphs', () => {
    const md = '## What is new\r\n\r\n- Faster **starts**\r\n- Fewer bugs\r\n\r\nThanks to all.\nSee you.\n\n### Fixes\n* one';
    expect(parseNotes(md)).toEqual([
      { kind: 'heading', level: 2, text: 'What is new' },
      {
        kind: 'list',
        items: [[{ text: 'Faster ' }, { text: 'starts', bold: true }], [{ text: 'Fewer bugs' }]],
      },
      { kind: 'paragraph', segments: [{ text: 'Thanks to all. See you.' }] },
      { kind: 'heading', level: 3, text: 'Fixes' },
      { kind: 'list', items: [[{ text: 'one' }]] },
    ]);
  });
  it('is empty for an empty description', () => {
    expect(parseNotes('  \n ')).toEqual([]);
  });
});
