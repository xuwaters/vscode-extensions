import { describe, expect, it } from 'vitest';
import { parseWebviewMessage } from './messages.js';

describe('parseWebviewMessage', () => {
  it('accepts every well-formed variant', () => {
    expect(parseWebviewMessage({ type: 'ready' })).toEqual({ type: 'ready' });
    expect(parseWebviewMessage({ type: 'openText' })).toEqual({ type: 'openText' });
    expect(parseWebviewMessage({ type: 'openLine', line: 12 })).toEqual({
      type: 'openLine',
      line: 12,
    });
    expect(parseWebviewMessage({ type: 'copy', text: 'x' })).toEqual({ type: 'copy', text: 'x' });
    expect(parseWebviewMessage({ type: 'error', message: 'boom' })).toEqual({
      type: 'error',
      message: 'boom',
    });
  });

  it('rejects junk', () => {
    expect(parseWebviewMessage(null)).toBeNull();
    expect(parseWebviewMessage('ready')).toBeNull();
    expect(parseWebviewMessage({})).toBeNull();
    expect(parseWebviewMessage({ type: 'run', command: 'rm -rf' })).toBeNull();
  });

  it('bounds numbers and strings', () => {
    expect(parseWebviewMessage({ type: 'openLine', line: -1 })).toBeNull();
    expect(parseWebviewMessage({ type: 'openLine', line: 1.5 })).toBeNull();
    expect(parseWebviewMessage({ type: 'openLine', line: 1e12 })).toBeNull();
    expect(parseWebviewMessage({ type: 'copy', text: 42 })).toBeNull();
    const long = parseWebviewMessage({ type: 'error', message: 'x'.repeat(10_000) });
    expect(long && long.type === 'error' ? long.message.length : 0).toBe(4_000);
  });
});
