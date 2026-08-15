import { describe, expect, it } from 'vitest';
import { isWebviewToHost } from './messages';

describe('isWebviewToHost', () => {
  it('accepts every valid message shape', () => {
    expect(isWebviewToHost({ type: 'ready' })).toBe(true);
    expect(isWebviewToHost({ type: 'revealLine', line: 3 })).toBe(true);
    expect(isWebviewToHost({ type: 'jumpToLine', line: 0 })).toBe(true);
    expect(isWebviewToHost({ type: 'navigate', direction: 'back' })).toBe(true);
    expect(isWebviewToHost({ type: 'navigate', direction: 'forward' })).toBe(
      true,
    );
    expect(isWebviewToHost({ type: 'openSource' })).toBe(true);
    expect(isWebviewToHost({ type: 'openLink', href: 'https://x' })).toBe(true);
    expect(isWebviewToHost({ type: 'setTheme', theme: 'github-dark' })).toBe(
      true,
    );
    expect(isWebviewToHost({ type: 'setFont', font: 'monospace' })).toBe(true);
    expect(
      isWebviewToHost({ type: 'toggleTask', line: 4, checked: true }),
    ).toBe(true);
    expect(
      isWebviewToHost({ type: 'error', message: 'm', context: 'c' }),
    ).toBe(true);
  });

  it('rejects malformed and unknown messages', () => {
    expect(isWebviewToHost(null)).toBe(false);
    expect(isWebviewToHost('ready')).toBe(false);
    expect(isWebviewToHost({ type: 'evil' })).toBe(false);
    expect(isWebviewToHost({ type: 'revealLine', line: 'x' })).toBe(false);
    expect(isWebviewToHost({ type: 'revealLine', line: -1 })).toBe(false);
    expect(isWebviewToHost({ type: 'revealLine', line: NaN })).toBe(false);
    expect(isWebviewToHost({ type: 'navigate' })).toBe(false);
    expect(isWebviewToHost({ type: 'navigate', direction: 'sideways' })).toBe(
      false,
    );
    expect(isWebviewToHost({ type: 'openLink' })).toBe(false);
    expect(isWebviewToHost({ type: 'setTheme', theme: 'solarized' })).toBe(
      false,
    );
    expect(isWebviewToHost({ type: 'setFont' })).toBe(false);
    expect(isWebviewToHost({ type: 'setFont', font: 'comic-sans' })).toBe(false);
    expect(isWebviewToHost({ type: 'toggleTask', line: 1 })).toBe(false);
    expect(isWebviewToHost({ type: 'error', message: 'm' })).toBe(false);
  });
});
