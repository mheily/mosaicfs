import { formatBytes, formatDate, formatRelative } from '@/lib/format';

describe('formatBytes', () => {
  it('returns "0 B" for zero', () => {
    expect(formatBytes(0)).toBe('0 B');
  });

  it('formats kilobytes', () => {
    expect(formatBytes(1024)).toBe('1.0 KB');
  });

  it('formats megabytes', () => {
    expect(formatBytes(2048576)).toBe('2.0 MB');
  });

  it('returns "—" for undefined', () => {
    expect(formatBytes(undefined as unknown as number)).toBe('—');
  });

  it('returns "—" for null', () => {
    expect(formatBytes(null as unknown as number)).toBe('—');
  });

  it('returns "—" for NaN', () => {
    expect(formatBytes(NaN)).toBe('—');
  });
});

describe('formatDate', () => {
  it('formats a valid ISO string', () => {
    const result = formatDate('2026-01-15T10:30:00Z');
    expect(result).not.toBe('—');
    expect(result.length).toBeGreaterThan(0);
  });

  it('returns "—" for undefined', () => {
    expect(formatDate(undefined as unknown as string)).toBe('—');
  });

  it('returns "—" for null', () => {
    expect(formatDate(null as unknown as string)).toBe('—');
  });

  it('returns "—" for empty string', () => {
    expect(formatDate('')).toBe('—');
  });

  it('returns "—" for invalid date string', () => {
    expect(formatDate('not-a-date')).toBe('—');
  });
});

describe('formatRelative', () => {
  it('returns "just now" for recent timestamps', () => {
    const now = new Date().toISOString();
    expect(formatRelative(now)).toBe('just now');
  });

  it('returns "—" for undefined', () => {
    expect(formatRelative(undefined as unknown as string)).toBe('—');
  });

  it('returns "—" for null', () => {
    expect(formatRelative(null as unknown as string)).toBe('—');
  });

  it('returns "—" for empty string', () => {
    expect(formatRelative('')).toBe('—');
  });

  it('returns "—" for invalid date', () => {
    expect(formatRelative('not-a-date')).toBe('—');
  });
});
