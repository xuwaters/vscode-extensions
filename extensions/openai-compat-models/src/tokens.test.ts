import { describe, expect, it } from 'vitest';
import { estimateMessageTokens, estimateTokens } from './tokens.js';

describe('estimateTokens', () => {
  it('returns 0 for empty input', () => {
    expect(estimateTokens('')).toBe(0);
  });

  it('estimates roughly chars/3.5 for English+code', () => {
    const text = 'function add(a, b) { return a + b; }'; // 36 chars
    const est = estimateTokens(text);
    expect(est).toBeGreaterThanOrEqual(10);
    expect(est).toBeLessThanOrEqual(13);
  });

  it('rounds up partial tokens', () => {
    expect(estimateTokens('a')).toBe(1);
  });
});

describe('estimateMessageTokens', () => {
  it('counts text-typed parts', () => {
    const tokens = estimateMessageTokens('user', [{ value: 'hello world' }]);
    expect(tokens).toBeGreaterThan(0);
  });

  it('handles plain string parts', () => {
    const tokens = estimateMessageTokens('user', ['hello world']);
    expect(tokens).toBeGreaterThan(0);
  });

  it('handles tool-call-style parts via JSON fallback', () => {
    const part = { name: 'lookup', input: { id: 42 } };
    const tokens = estimateMessageTokens('assistant', [part]);
    expect(tokens).toBeGreaterThan(0);
  });
});
