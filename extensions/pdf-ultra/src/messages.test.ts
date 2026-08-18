import { describe, expect, it } from 'vitest';
import {
  isAllowedLink,
  parseWebviewMessage,
  type ViewerPlace,
  type WebviewToHost,
} from './messages.js';

const place: ViewerPlace = {
  page: 3,
  zoom: 1.25,
  fit: 'fit-width',
  rotation: 90,
  inverted: false,
  outlineVisible: true,
  outlineWidth: 240,
  offsetRatio: 0.5,
};

describe('parsing a message from the webview', () => {
  // The webview is a hostile input boundary even though we wrote the code on
  // the other side of it: it is the surface an untrusted document runs against.

  it('accepts each variant', () => {
    const cases: WebviewToHost[] = [
      { type: 'ready' },
      { type: 'opened', pageCount: 12, title: undefined },
      { type: 'place', place },
      { type: 'openLink', href: 'https://example.com' },
      { type: 'pagePng', page: 1, data: 'AAAA' },
      { type: 'needBytes', reason: 'no url' },
      { type: 'failed', message: 'bad file' },
      { type: 'error', message: 'oops', context: 'draw' },
    ];
    for (const message of cases) {
      expect(parseWebviewMessage(message)).toEqual(message);
    }
  });

  it('rejects anything that is not a tagged object', () => {
    expect(parseWebviewMessage(null)).toBeNull();
    expect(parseWebviewMessage('ready')).toBeNull();
    expect(parseWebviewMessage(42)).toBeNull();
    expect(parseWebviewMessage({})).toBeNull();
    expect(parseWebviewMessage({ type: 'nonsense' })).toBeNull();
  });

  it('rejects a page count that is not a page count', () => {
    expect(parseWebviewMessage({ type: 'opened', pageCount: 0 })).toBeNull();
    expect(parseWebviewMessage({ type: 'opened', pageCount: 1.5 })).toBeNull();
    expect(parseWebviewMessage({ type: 'opened', pageCount: -1 })).toBeNull();
    expect(parseWebviewMessage({ type: 'opened', pageCount: Number.NaN })).toBeNull();
  });

  it('rejects a place with a field out of range', () => {
    expect(parseWebviewMessage({ type: 'place', place: { ...place, zoom: 0 } })).toBeNull();
    expect(parseWebviewMessage({ type: 'place', place: { ...place, rotation: 45 } })).toBeNull();
    expect(parseWebviewMessage({ type: 'place', place: { ...place, fit: 'huge' } })).toBeNull();
    expect(
      parseWebviewMessage({ type: 'place', place: { ...place, outlineWidth: -1 } }),
    ).toBeNull();
    expect(parseWebviewMessage({ type: 'place', place: null })).toBeNull();
  });

  it('rejects a PNG that would not survive a base64 decode', () => {
    // A corrupt payload here is a corrupt file on the reader's disk.
    expect(parseWebviewMessage({ type: 'pagePng', page: 1, data: 'AAA' })).toBeNull();
    expect(parseWebviewMessage({ type: 'pagePng', page: 1, data: 'AA A=' })).toBeNull();
    expect(parseWebviewMessage({ type: 'pagePng', page: 1, data: '../etc' })).toBeNull();
  });

  it('truncates the strings it does accept', () => {
    const long = 'x'.repeat(9000);
    const parsed = parseWebviewMessage({ type: 'error', message: long, context: long });
    expect(parsed).toEqual({
      type: 'error',
      message: 'x'.repeat(2000),
      context: 'x'.repeat(200),
    });
  });

  it('rejects an unreasonably long link rather than truncating it', () => {
    expect(
      parseWebviewMessage({ type: 'openLink', href: `https://e.com/${'a'.repeat(5000)}` }),
    ).toBeNull();
  });
});

describe('links the host will open', () => {
  it('allows the web and mail', () => {
    expect(isAllowedLink('https://example.com')).toBe(true);
    expect(isAllowedLink('http://example.com')).toBe(true);
    expect(isAllowedLink('mailto:someone@example.com')).toBe(true);
  });

  it('refuses schemes that would run or reveal something local', () => {
    // A PDF's link annotations are attacker-controlled strings, and `file:`
    // would hand anything on the machine to whatever the OS has registered.
    expect(isAllowedLink('file:///etc/passwd')).toBe(false);
    expect(isAllowedLink('javascript:alert(1)')).toBe(false);
    expect(isAllowedLink('vscode://ms-vscode.node-debug2/x')).toBe(false);
    expect(isAllowedLink('data:text/html,<script>')).toBe(false);
    expect(isAllowedLink('not a url')).toBe(false);
  });
});
