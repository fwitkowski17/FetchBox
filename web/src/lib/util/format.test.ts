import { describe, expect, it } from 'vitest';
import {
  durationParts,
  etaSeconds,
  formatBytes,
  formatDate,
  formatPercent,
  formatSpeed,
  fraction,
  hostOf,
  isHttpUrl,
  joinSpeed,
  splitSpeed,
} from './format';

describe('formatBytes', () => {
  it('uses binary steps with one decimal below 10', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(999)).toBe('999 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(10 * 1024)).toBe('10 KB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
    expect(formatBytes(3.5 * 1024 ** 3)).toBe('3.5 GB');
  });

  it('never shows 1024 of a unit', () => {
    expect(formatBytes(1024 * 1024 - 1)).toBe('1.0 MB');
    expect(formatBytes(1024 ** 3 - 1)).toBe('1.0 GB');
  });

  it('stops at TB and survives garbage', () => {
    expect(formatBytes(5 * 1024 ** 4)).toBe('5.0 TB');
    expect(formatBytes(5000 * 1024 ** 4)).toBe('5000 TB');
    expect(formatBytes(-1)).toBe('—');
    expect(formatBytes(Number.NaN)).toBe('—');
  });

  it('formats speed', () => {
    expect(formatSpeed(2 * 1024 * 1024)).toBe('2.0 MB/s');
  });

  it('uses the decimal separator of the language and never groups digits', () => {
    expect(formatBytes(1536, 'pl')).toBe('1,5 KB');
    expect(formatBytes(1536, 'en')).toBe('1.5 KB');
    expect(formatSpeed(2.5 * 1024 * 1024, 'pl')).toBe('2,5 MB/s');
    expect(formatBytes(1000 * 1024, 'pl')).toBe('1000 KB');
    expect(formatBytes(1000 * 1024, 'en')).toBe('1000 KB');
  });
});

describe('formatDate', () => {
  it('follows the language', () => {
    const t = Date.UTC(2026, 9, 14, 12, 0) / 1000;
    expect(formatDate(t, 'en')).toMatch(/Oct/);
    expect(formatDate(t, 'pl')).toMatch(/paź/);
  });
});

describe('durationParts / etaSeconds', () => {
  it('picks the unit pattern and pads where it reads better', () => {
    expect(durationParts(0)).toEqual({ unit: 's', values: { s: '0' } });
    expect(durationParts(45)).toEqual({ unit: 's', values: { s: '45' } });
    expect(durationParts(65)).toEqual({ unit: 'ms', values: { m: '1', s: '05' } });
    expect(durationParts(3 * 3600 + 7 * 60)).toEqual({ unit: 'hm', values: { h: '3', m: '07' } });
    expect(durationParts(30 * 3600)).toEqual({ unit: 'dh', values: { d: '1', h: '6' } });
  });

  it('rounds, and gives up on nonsense and on 100 days or more', () => {
    expect(durationParts(59.6)).toEqual({ unit: 'ms', values: { m: '1', s: '00' } });
    expect(durationParts(200 * 86_400)).toBeNull();
    expect(durationParts(-5)).toBeNull();
    expect(durationParts(Number.NaN)).toBeNull();
  });

  it('is unknown without size or speed, or when done', () => {
    expect(etaSeconds(10, null, 5)).toBeNull();
    expect(etaSeconds(10, undefined, 5)).toBeNull();
    expect(etaSeconds(10, 100, 0)).toBeNull();
    expect(etaSeconds(100, 100, 5)).toBeNull();
    expect(etaSeconds(50, 100, 10)).toBe(5);
  });
});

describe('fraction', () => {
  it('is null for unknown size and clamps to 0..1', () => {
    expect(fraction(5, null)).toBeNull();
    expect(fraction(5, 0)).toBeNull();
    expect(fraction(50, 200)).toBe(0.25);
    expect(fraction(300, 200)).toBe(1);
    expect(fraction(-1, 200)).toBe(0);
  });

  it('formats percent without rounding up to 100 early', () => {
    expect(formatPercent(null)).toBe('');
    expect(formatPercent(0.999)).toBe('99%');
    expect(formatPercent(1)).toBe('100%');
  });
});

describe('speed limit fields', () => {
  it('splits into a friendly unit and joins back', () => {
    expect(splitSpeed(0)).toEqual({ value: 0, unit: 'KB' });
    expect(splitSpeed(512 * 1024)).toEqual({ value: 512, unit: 'KB' });
    expect(splitSpeed(5 * 1024 * 1024)).toEqual({ value: 5, unit: 'MB' });
    expect(splitSpeed(1536 * 1024)).toEqual({ value: 1536, unit: 'KB' });
    // Bits are decimal: 1 Mbit/s = 125 000 B/s. What was typed in bits comes back in bits.
    expect(joinSpeed(8, 'Mbit')).toBe(1_000_000);
    expect(joinSpeed(512, 'Kbit')).toBe(64_000);
    expect(splitSpeed(12_500_000)).toEqual({ value: 100, unit: 'Mbit' });
    expect(splitSpeed(187_500)).toEqual({ value: 1500, unit: 'Kbit' });
    expect(splitSpeed(250_000)).toEqual({ value: 2, unit: 'Mbit' });
    for (const unit of ['KB', 'MB', 'Kbit', 'Mbit'] as const) {
      for (const value of [1, 3, 64, 250, 999]) {
        expect(splitSpeed(joinSpeed(value, unit))).toEqual({ value, unit });
      }
    }
    for (const bytes of [0, 1024, 300 * 1024, 5 * 1024 * 1024, 123_456]) {
      const { value, unit } = splitSpeed(bytes);
      expect(Math.abs(joinSpeed(value, unit) - bytes)).toBeLessThanOrEqual(6);
    }
  });

  it('treats empty, zero and negative as unlimited', () => {
    expect(joinSpeed(0, 'MB')).toBe(0);
    expect(joinSpeed(-3, 'KB')).toBe(0);
    expect(joinSpeed(Number.NaN, 'KB')).toBe(0);
    expect(joinSpeed(2, 'MB')).toBe(2 * 1024 * 1024);
  });
});

describe('hostOf', () => {
  it('extracts the host or falls back', () => {
    expect(hostOf('https://example.com:8080/a/b.zip?x=1')).toBe('example.com:8080');
    expect(hostOf('not a url')).toBe('not a url');
  });
});

describe('isHttpUrl', () => {
  it('accepts only http(s) URLs', () => {
    expect(isHttpUrl('https://example.com/a.zip')).toBe(true);
    expect(isHttpUrl('  http://nas/file  ')).toBe(true);
    expect(isHttpUrl('ftp://example.com/a')).toBe(false);
    expect(isHttpUrl('file:///etc/passwd')).toBe(false);
    expect(isHttpUrl('example.com/a.zip')).toBe(false);
    expect(isHttpUrl('')).toBe(false);
  });
});

describe('speed display unit', () => {
  it('keeps the automatic byte format as it was', () => {
    expect(formatSpeed(0)).toBe('0 B/s');
    expect(formatSpeed(1_500_000)).toBe('1.4 MB/s');
    expect(formatSpeed(1_500_000, 'en', 'auto')).toBe('1.4 MB/s');
  });

  it('picks bit units by itself, decimal as networks quote them', () => {
    expect(formatSpeed(0, 'en', 'bits')).toBe('0 bit/s');
    expect(formatSpeed(125, 'en', 'bits')).toBe('1.00 Kbit/s');
    expect(formatSpeed(1_500_000, 'en', 'bits')).toBe('12.0 Mbit/s');
    expect(formatSpeed(12_500_000, 'en', 'bits')).toBe('100 Mbit/s');
    expect(formatSpeed(125_000_000, 'en', 'bits')).toBe('1.00 Gbit/s');
    // 999.96 Kbit/s must not be shown as "1000 Kbit/s".
    expect(formatSpeed(124_995, 'en', 'bits')).toBe('1.00 Mbit/s');
  });

  it('shows one fixed unit when asked to', () => {
    expect(formatSpeed(1_048_576, 'en', 'KB')).toBe('1024 KB/s');
    expect(formatSpeed(5 * 1_048_576, 'en', 'MB')).toBe('5.00 MB/s');
    expect(formatSpeed(40 * 1024, 'en', 'MB')).toBe('0.04 MB/s');
    expect(formatSpeed(12_500_000, 'en', 'Mbit')).toBe('100 Mbit/s');
    expect(formatSpeed(64_000, 'en', 'Kbit')).toBe('512 Kbit/s');
    expect(formatSpeed(Number.NaN, 'en', 'Mbit')).toBe('—');
  });

  it('uses the number format of the language', () => {
    expect(formatSpeed(1_500_000, 'pl', 'bits')).toBe('12,0 Mbit/s');
  });
});

