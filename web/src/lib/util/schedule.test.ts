import { describe, expect, it } from 'vitest';
import { formatWhen, isZone, minutesToTime, nextOccurrence, timeToMinutes } from './schedule';

describe('times of the day', () => {
  it('converts both ways', () => {
    expect(minutesToTime(0)).toBe('00:00');
    expect(minutesToTime(390)).toBe('06:30');
    expect(minutesToTime(24 * 60 - 1)).toBe('23:59');
    expect(timeToMinutes('06:30')).toBe(390);
    expect(timeToMinutes(' 23:59 ')).toBe(1439);
  });
  it('refuses what is not a time', () => {
    for (const bad of ['', '6:30', '24:00', '12:60', 'noon', '12:3']) expect(timeToMinutes(bad)).toBeNull();
  });
  it('keeps a minute count inside the day', () => {
    expect(minutesToTime(-5)).toBe('00:00');
    expect(minutesToTime(5000)).toBe('23:59');
  });
});

describe('nextOccurrence', () => {
  const now = new Date(2026, 0, 10, 22, 0, 0);
  it('is today when the time is still ahead', () => {
    const at = nextOccurrence('23:30', now);
    expect(at?.getDate()).toBe(10);
    expect([at?.getHours(), at?.getMinutes()]).toEqual([23, 30]);
  });
  it('is tomorrow when it has passed, or is this very minute', () => {
    expect(nextOccurrence('06:00', now)?.getDate()).toBe(11);
    expect(nextOccurrence('22:00', now)?.getDate()).toBe(11);
  });
  it('is nothing for a broken time', () => {
    expect(nextOccurrence('later', now)).toBeNull();
  });
});

describe('formatWhen', () => {
  const now = new Date(2026, 0, 10, 12, 0, 0).getTime();
  const sec = (d: Date) => Math.floor(d.getTime() / 1000);
  it('says only the time for today', () => {
    expect(formatWhen(sec(new Date(2026, 0, 10, 23, 5)), 'en', now)).toMatch(/11:05/);
  });
  it('names tomorrow in the language', () => {
    expect(formatWhen(sec(new Date(2026, 0, 11, 1, 0)), 'en', now)).toMatch(/^tomorrow .*01:00/);
    expect(formatWhen(sec(new Date(2026, 0, 11, 1, 0)), 'pl', now)).toMatch(/^jutro /);
  });
  it('gives the date for later', () => {
    expect(formatWhen(sec(new Date(2026, 0, 14, 1, 0)), 'en', now)).toMatch(/Jan 14, 2026/);
  });
});

describe('isZone', () => {
  it('knows real zones and refuses nonsense', () => {
    expect(isZone('Europe/Warsaw')).toBe(true);
    expect(isZone('UTC')).toBe(true);
    expect(isZone('Mars/Olympus')).toBe(false);
    expect(isZone('')).toBe(false);
  });
});
