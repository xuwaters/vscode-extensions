import { describe, expect, it } from 'vitest';
import { decodeBase64, encodeBase64 } from './base64.js';

describe('encodeBase64', () => {
  it('encodes plain ASCII text', () => {
    expect(encodeBase64('Hello, World!')).toBe('SGVsbG8sIFdvcmxkIQ==');
  });

  it('encodes empty string', () => {
    expect(encodeBase64('')).toBe('');
  });

  it('encodes UTF-8 text', () => {
    expect(encodeBase64('你好')).toBe('5L2g5aW9');
  });

  it('round-trips with decodeBase64', () => {
    const original = 'round-trip test 123!';
    const encoded = encodeBase64(original);
    const result = decodeBase64(encoded);
    expect(result).toEqual({ ok: true, value: original });
  });
});

describe('decodeBase64', () => {
  it('decodes a valid base64 string', () => {
    expect(decodeBase64('SGVsbG8sIFdvcmxkIQ==')).toEqual({ ok: true, value: 'Hello, World!' });
  });

  it('decodes with leading/trailing whitespace', () => {
    expect(decodeBase64('  SGVsbG8=  ')).toEqual({ ok: true, value: 'Hello' });
  });

  it('decodes base64 without padding', () => {
    expect(decodeBase64('SGVsbG8=')).toEqual({ ok: true, value: 'Hello' });
  });

  it('returns error for invalid base64 characters', () => {
    const result = decodeBase64('not-valid-base64!!!');
    expect(result.ok).toBe(false);
  });

  it('decodes empty string', () => {
    expect(decodeBase64('')).toEqual({ ok: true, value: '' });
  });

  it('returns error for string with spaces (not trimmed)', () => {
    const result = decodeBase64('SGVs bG8=');
    expect(result.ok).toBe(false);
  });
});
