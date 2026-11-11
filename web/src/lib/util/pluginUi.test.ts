import { describe, expect, it } from 'vitest';
import type { ActionResult, PluginField, PluginFieldView } from '../api/types';
import {
  actionOutcome,
  hasChange,
  iconOrDefault,
  isSafeExternalUrl,
  isShown,
  settingsChange,
  tr,
  validateValues,
  valuesFromFields,
  valuesFromViews,
} from './pluginUi';

function field(over: Partial<PluginField> & Pick<PluginField, 'key' | 'type'>): PluginField {
  return { ...over };
}

function view(f: PluginField, value?: PluginFieldView['value'], has = value !== undefined): PluginFieldView {
  return { field: f, has_value: has, value: value ?? null };
}

describe('tr', () => {
  const i18n = { pl: { Hello: 'Cześć' }, 'pt-BR': { Hello: 'Olá' } };
  it('uses the exact locale, then the bare language, then the text', () => {
    expect(tr(i18n, 'pl-PL', 'Hello')).toBe('Cześć');
    expect(tr(i18n, 'pt-BR', 'Hello')).toBe('Olá');
    expect(tr(i18n, 'de', 'Hello')).toBe('Hello');
    expect(tr(i18n, 'pl', 'Other')).toBe('Other');
    expect(tr(i18n, 'pl', '')).toBe('');
  });
});

describe('initial values', () => {
  it('prefers the stored value, then the default, and leaves secrets empty', () => {
    const views = [
      view(field({ key: 'a', type: 'text', default: 'x' }), 'stored'),
      view(field({ key: 'b', type: 'text', default: 'x' })),
      view(field({ key: 'c', type: 'toggle' })),
      view(field({ key: 'd', type: 'secret' }), undefined, true),
      view(field({ key: 'e', type: 'info' })),
    ];
    expect(valuesFromViews(views)).toEqual({ a: 'stored', b: 'x', c: false, d: '' });
  });

  it('a dialog starts on the defaults', () => {
    expect(
      valuesFromFields([
        field({ key: 'n', type: 'number', default: 3 }),
        field({ key: 'o', type: 'output' }),
        field({ key: 's', type: 'select' }),
      ]),
    ).toEqual({ n: 3, s: '' });
  });
});

describe('show_if', () => {
  const dependent = field({ key: 'url', type: 'text', show_if: { key: 'mode', equals: 'custom' } });
  it('follows the other field', () => {
    expect(isShown(dependent, { mode: 'custom' })).toBe(true);
    expect(isShown(dependent, { mode: 'auto' })).toBe(false);
    expect(isShown(field({ key: 'x', type: 'text' }), {})).toBe(true);
  });
});

describe('validateValues', () => {
  const fields = [
    field({ key: 'name', type: 'text', required: true }),
    field({ key: 'n', type: 'number', min: 1, max: 10 }),
    field({ key: 'key', type: 'secret', required: true }),
    field({ key: 'hidden', type: 'text', required: true, show_if: { key: 'name', equals: 'zzz' } }),
  ];
  it('finds what is missing or out of range', () => {
    expect(validateValues(fields, { name: ' ', n: 0, key: '', hidden: '' })).toEqual({
      name: { code: 'required' },
      n: { code: 'min', limit: 1 },
      key: { code: 'required' },
    });
    expect(validateValues(fields, { name: 'a', n: 11, key: 'k' })).toEqual({ n: { code: 'max', limit: 10 } });
    expect(validateValues(fields, { name: 'a', n: 'abc', key: 'k' })).toEqual({ n: { code: 'number' } });
  });
  it('an empty secret is fine when one is stored', () => {
    expect(validateValues(fields, { name: 'a', n: '', key: '' }, new Set(['key']))).toEqual({});
  });
});

describe('settingsChange', () => {
  const views = [
    view(field({ key: 'url', type: 'text' }), 'https://a'),
    view(field({ key: 'n', type: 'number', default: 2 })),
    view(field({ key: 'on', type: 'toggle', default: false })),
    view(field({ key: 'tok', type: 'secret' }), undefined, true),
  ];

  it('is empty when nothing changed', () => {
    const c = settingsChange(views, { url: 'https://a', n: 2, on: false, tok: '' });
    expect(hasChange(c)).toBe(false);
  });

  it('sends only what differs, as numbers', () => {
    const c = settingsChange(views, { url: 'https://b', n: '5', on: true, tok: '' });
    expect(c).toEqual({ values: { url: 'https://b', n: 5, on: true }, clear: [] });
  });

  it('clears an emptied field that had a value, and a secret only when asked', () => {
    expect(settingsChange(views, { url: '', n: 2, on: false, tok: '' }).clear).toEqual(['url']);
    expect(settingsChange(views, { url: 'https://a', n: 2, on: false, tok: 'new' }).values).toEqual({ tok: 'new' });
    expect(settingsChange(views, { url: 'https://a', n: 2, on: false, tok: '' }, new Set(['tok'])).clear).toEqual(['tok']);
  });

  it('leaves hidden fields alone', () => {
    const v = [view(field({ key: 'x', type: 'text', show_if: { key: 'on', equals: true } }), 'old'), view(field({ key: 'on', type: 'toggle' }), false)];
    expect(hasChange(settingsChange(v, { x: '', on: false }))).toBe(false);
  });
});

describe('isSafeExternalUrl', () => {
  it('opens only web addresses', () => {
    expect(isSafeExternalUrl('https://example.com/a')).toBe(true);
    expect(isSafeExternalUrl('http://192.168.1.2:8080')).toBe(true);
    expect(isSafeExternalUrl('javascript:alert(1)')).toBe(false);
    expect(isSafeExternalUrl('data:text/html,hi')).toBe(false);
    expect(isSafeExternalUrl('file:///etc/passwd')).toBe(false);
    expect(isSafeExternalUrl('nonsense')).toBe(false);
    expect(isSafeExternalUrl(null)).toBe(false);
  });
});

describe('iconOrDefault', () => {
  it('takes only known names', () => {
    expect(iconOrDefault('video', ['video', 'other'], 'other')).toBe('video');
    expect(iconOrDefault('<script>', ['video', 'other'], 'other')).toBe('other');
    expect(iconOrDefault(undefined, ['video', 'other'], 'other')).toBe('other');
  });
});

describe('actionOutcome', () => {
  it('reads a successful answer', () => {
    const r: ActionResult = { ok: true, message: 'Sent', close: true, set: { out: 'x' } };
    expect(actionOutcome(r, 'Failed')).toEqual({
      ok: true,
      notice: { level: 'success', message: 'Sent' },
      close: true,
      openDialog: null,
      set: { out: 'x' },
    });
  });
  it('a silent success shows nothing, a silent failure says it failed', () => {
    expect(actionOutcome({ ok: true }, 'Failed').notice).toBeNull();
    expect(actionOutcome({ ok: false }, 'Failed').notice).toEqual({ level: 'error', message: 'Failed' });
  });
  it('keeps the level the plugin chose and the dialog it opens', () => {
    const o = actionOutcome({ ok: true, message: 'Careful', level: 'warning', open_dialog: 'next' }, 'F');
    expect(o.notice?.level).toBe('warning');
    expect(o.openDialog).toBe('next');
  });
});
