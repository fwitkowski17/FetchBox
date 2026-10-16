// Release descriptions are Markdown written by the project's author on GitHub. They are shown as
// plain structure (headings, lists, paragraphs) made of text nodes, never as HTML.

export interface Segment {
  text: string;
  bold?: boolean;
  code?: boolean;
}

export type NoteBlock =
  | { kind: 'heading'; level: number; text: string }
  | { kind: 'list'; items: Segment[][] }
  | { kind: 'paragraph'; segments: Segment[] };

/** `**bold**` and `` `code` `` become segments; a link `[text](url)` keeps only its text. */
export function inline(text: string): Segment[] {
  const plain = text.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1');
  const out: Segment[] = [];
  const pattern = /\*\*([^*]+)\*\*|`([^`]+)`/g;
  let last = 0;
  for (const m of plain.matchAll(pattern)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ text: plain.slice(last, at) });
    if (m[1] !== undefined) out.push({ text: m[1], bold: true });
    else if (m[2] !== undefined) out.push({ text: m[2], code: true });
    last = at + m[0].length;
  }
  if (last < plain.length) out.push({ text: plain.slice(last) });
  return out.length > 0 ? out : [{ text: plain }];
}

/** The blocks of a Markdown description: `#` headings, `-`/`*` lists, paragraphs split by blank lines. */
export function parseNotes(markdown: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  let paragraph: string[] = [];
  let list: Segment[][] | null = null;

  const endParagraph = (): void => {
    if (paragraph.length > 0) blocks.push({ kind: 'paragraph', segments: inline(paragraph.join(' ')) });
    paragraph = [];
  };
  const endList = (): void => {
    if (list) blocks.push({ kind: 'list', items: list });
    list = null;
  };

  for (const raw of markdown.replace(/\r\n?/g, '\n').split('\n')) {
    const line = raw.trim();
    const heading = /^(#{1,6})\s+(.*\S)\s*#*$/.exec(line);
    const item = /^[-*+]\s+(.*\S)$/.exec(line);
    if (line === '') {
      endParagraph();
      endList();
    } else if (heading?.[1] !== undefined && heading[2] !== undefined) {
      endParagraph();
      endList();
      blocks.push({ kind: 'heading', level: heading[1].length, text: heading[2] });
    } else if (item?.[1] !== undefined) {
      endParagraph();
      (list ??= []).push(inline(item[1]));
    } else {
      endList();
      paragraph.push(line);
    }
  }
  endParagraph();
  endList();
  return blocks;
}
