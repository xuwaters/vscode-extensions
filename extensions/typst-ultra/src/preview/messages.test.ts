import { describe, expect, it } from 'vitest';
import { isAllowedLink, parseWebviewMessage } from './messages.js';

/**
 * A webview is an input boundary even though we wrote both sides. These guards
 * are the lesson RFC 009 recorded from markdown-preview-enhanced's CVE history,
 * and they apply identically here.
 */
describe('webview message validation', () => {
  it('accepts a well-formed viewport message', () => {
    expect(
      parseWebviewMessage({
        type: 'viewport',
        first: 0,
        last: 3,
        known: { 0: '0123456789abcdef' },
        zoom: 1,
      }),
    ).toEqual({
      type: 'viewport',
      first: 0,
      last: 3,
      known: { 0: '0123456789abcdef' },
      zoom: 1,
    });
  });

  it('rejects a viewport whose range runs backwards', () => {
    expect(
      parseWebviewMessage({ type: 'viewport', first: 5, last: 2, known: {}, zoom: 1 }),
    ).toBeNull();
  });

  it('rejects a viewport with an absurd zoom', () => {
    for (const zoom of [0, -1, 1000, NaN, '2']) {
      expect(
        parseWebviewMessage({ type: 'viewport', first: 0, last: 1, known: {}, zoom }),
      ).toBeNull();
    }
  });

  it('rejects a hash that is not one of ours', () => {
    for (const hash of ['', 'zzzz', '0123456789abcde', '<script>', 123]) {
      expect(
        parseWebviewMessage({
          type: 'viewport',
          first: 0,
          last: 0,
          known: { 0: hash },
          zoom: 1,
        }),
      ).toBeNull();
    }
  });

  it('rejects a negative or absurd page index', () => {
    expect(
      parseWebviewMessage({ type: 'click', page: -1, xPt: 0, yPt: 0 }),
    ).toBeNull();
    expect(
      parseWebviewMessage({ type: 'click', page: 1e9, xPt: 0, yPt: 0 }),
    ).toBeNull();
    expect(
      parseWebviewMessage({ type: 'click', page: 1.5, xPt: 0, yPt: 0 }),
    ).toBeNull();
  });

  it('rejects non-finite coordinates', () => {
    expect(
      parseWebviewMessage({ type: 'click', page: 0, xPt: NaN, yPt: 0 }),
    ).toBeNull();
    expect(
      parseWebviewMessage({ type: 'click', page: 0, xPt: 0, yPt: Infinity }),
    ).toBeNull();
  });

  it('rejects an unknown message type', () => {
    expect(parseWebviewMessage({ type: 'evalThis', code: '1' })).toBeNull();
    expect(parseWebviewMessage({ type: '__proto__' })).toBeNull();
    expect(parseWebviewMessage(null)).toBeNull();
    expect(parseWebviewMessage('ready')).toBeNull();
    expect(parseWebviewMessage(42)).toBeNull();
  });

  it('bounds the strings it accepts', () => {
    expect(
      parseWebviewMessage({ type: 'openLink', href: 'x'.repeat(5000) }),
    ).toBeNull();

    const error = parseWebviewMessage({
      type: 'error',
      message: 'y'.repeat(5000),
      context: 'z'.repeat(500),
    });
    expect(error).toEqual({
      type: 'error',
      message: 'y'.repeat(2000),
      context: 'z'.repeat(200),
    });
  });

  it('rejects an out-of-range zoom or an unknown fit mode', () => {
    expect(
      parseWebviewMessage({ type: 'state', zoom: 0, fit: 'width', inverted: false }),
    ).toBeNull();
    expect(
      parseWebviewMessage({ type: 'state', zoom: 100, fit: 'width', inverted: false }),
    ).toBeNull();
    expect(
      parseWebviewMessage({ type: 'state', zoom: 1, fit: 'cover', inverted: false }),
    ).toBeNull();
    expect(
      parseWebviewMessage({ type: 'state', zoom: 1, fit: 'width', inverted: false }),
    ).toEqual({ type: 'state', zoom: 1, fit: 'width', inverted: false });
  });

  // The toolbar's two buttons carry no payload, so the whole guard is the name:
  // anything extra is dropped rather than passed on to a command.
  it('accepts the toolbar buttons and nothing they might smuggle', () => {
    expect(parseWebviewMessage({ type: 'export' })).toEqual({ type: 'export' });
    expect(parseWebviewMessage({ type: 'openSource' })).toEqual({
      type: 'openSource',
    });
    expect(
      parseWebviewMessage({ type: 'export', uri: 'file:///etc/passwd' }),
    ).toEqual({ type: 'export' });
  });
});

describe('the link allowlist', () => {
  it('allows the three schemes a document has any business using', () => {
    expect(isAllowedLink('https://typst.app')).toBe(true);
    expect(isAllowedLink('http://example.com')).toBe(true);
    expect(isAllowedLink('mailto:someone@example.com')).toBe(true);
  });

  it('refuses everything else', () => {
    for (const href of [
      'javascript:alert(1)',
      'vscode://ms-vscode.remote/x',
      'file:///etc/passwd',
      'data:text/html,<script>alert(1)</script>',
      'not a url',
      '',
    ]) {
      expect(isAllowedLink(href), href).toBe(false);
    }
  });
});
