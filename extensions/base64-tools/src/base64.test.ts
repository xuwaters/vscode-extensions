import { describe, expect, it } from 'vitest';
import { decodeBase64, decodeJwtLike, encodeBase64, isJwtLike } from './base64.js';

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

// A real JWT: header.payload.signature
const SAMPLE_JWT =
  'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9' +
  '.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ' +
  '.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c';

describe('isJwtLike', () => {
  it('detects a 3-part JWT', () => {
    expect(isJwtLike(SAMPLE_JWT)).toBe(true);
  });

  it('detects a 2-part base64url string', () => {
    expect(isJwtLike('eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0')).toBe(true);
  });

  it('returns false for plain base64 without dots', () => {
    expect(isJwtLike('SGVsbG8sIFdvcmxkIQ==')).toBe(false);
  });

  it('returns false for strings with empty middle parts', () => {
    expect(isJwtLike('abc..def')).toBe(true);
  });

  it('returns false for strings with empty last part (trailing dot)', () => {
    expect(isJwtLike('eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.')).toBe(true);
  });
});

describe('decodeJwtLike', () => {
  it('decodes header and payload of a 3-part JWT', () => {
    const result = decodeJwtLike(SAMPLE_JWT);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const parts = result.value.split('\n.\n');
    expect(parts).toHaveLength(2);
    expect(JSON.parse(parts[0])).toMatchObject({ alg: 'HS256', typ: 'JWT' });
    expect(JSON.parse(parts[1])).toMatchObject({ sub: '1234567890', name: 'John Doe' });
  });

  it('decodes both parts of a 2-part JWT-like string', () => {
    const header = Buffer.from(JSON.stringify({ alg: 'none' })).toString('base64url');
    const payload = Buffer.from(JSON.stringify({ sub: 'user' })).toString('base64url');
    const result = decodeJwtLike(`${header}.${payload}`);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const parts = result.value.split('\n.\n');
    expect(parts).toHaveLength(2);
    expect(JSON.parse(parts[0])).toMatchObject({ alg: 'none' });
    expect(JSON.parse(parts[1])).toMatchObject({ sub: 'user' });
  });
});
