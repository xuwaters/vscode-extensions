// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CodeBlockPrefs } from '../../src/messages';
import {
  addCodeActions,
  applyCodeBlockPrefs,
  installCodeActions,
  readStampedCodeBlockPrefs,
  splitLines,
} from './codeBlocks';

function codeOf(html: string): HTMLElement {
  document.body.innerHTML = `<pre><code>${html}</code></pre>`;
  return document.querySelector('code') as HTMLElement;
}

function lines(code: HTMLElement): string[] {
  return Array.from(code.children).map((l) => {
    expect(l.className).toBe('code-line');
    return l.innerHTML;
  });
}

describe('splitLines', () => {
  it('splits plain text into one span per line, keeping the newlines', () => {
    const code = codeOf('a\nb\n\nc\n');
    splitLines(code);
    expect(lines(code)).toEqual(['a\n', 'b\n', '\n', 'c\n']);
    expect(code.textContent).toBe('a\nb\n\nc\n');
    expect(code.style.getPropertyValue('--line-digits')).toBe('1');
  });

  it('keeps a last line that has no closing newline', () => {
    const code = codeOf('a\nb');
    splitLines(code);
    expect(lines(code)).toEqual(['a\n', 'b']);
  });

  it('closes and reopens a token span that crosses a line break', () => {
    const html =
      '<span class="k">let</span> x = <span class="s">"a\nb"</span>;\ny\n';
    const code = codeOf(html);
    const before = code.textContent;
    splitLines(code);
    expect(lines(code)).toEqual([
      '<span class="k">let</span> x = <span class="s">"a\n</span>',
      '<span class="s">b"</span>;\n',
      'y\n',
    ]);
    expect(code.textContent).toBe(before);
  });

  it('carries nested spans across several lines', () => {
    const code = codeOf('<span class="a">1<span class="b">2\n3\n4</span>5</span>\n');
    splitLines(code);
    expect(lines(code)).toEqual([
      '<span class="a">1<span class="b">2\n</span></span>',
      '<span class="a"><span class="b">3\n</span></span>',
      '<span class="a"><span class="b">4</span>5</span>\n',
    ]);
  });

  it('sizes the gutter for the longest line number', () => {
    const code = codeOf('x\n'.repeat(120));
    splitLines(code);
    expect(code.children).toHaveLength(120);
    expect(code.style.getPropertyValue('--line-digits')).toBe('3');
  });

  it('runs once per block', () => {
    const code = codeOf('a\nb\n');
    splitLines(code);
    splitLines(code);
    expect(lines(code)).toEqual(['a\n', 'b\n']);
  });
});

describe('addCodeActions', () => {
  beforeEach(() => {
    document.body.innerHTML =
      '<div id="c"><pre><code>a\nb\n</code></pre><pre>no code</pre></div>';
  });

  it('adds Wrap, Lines and Copy to fences only, once', () => {
    const root = document.getElementById('c') as HTMLElement;
    addCodeActions([root]);
    addCodeActions([root]);
    const bars = root.querySelectorAll('.code-actions');
    expect(bars).toHaveLength(1);
    const labels = Array.from(bars[0].querySelectorAll('button')).map(
      (b) => b.textContent,
    );
    expect(labels).toEqual(['Wrap', 'Lines', 'Copy']);
    for (const b of bars[0].querySelectorAll('button')) {
      expect(b.type).toBe('button');
    }
    expect(root.querySelectorAll('.code-line')).toHaveLength(2);
  });

  it('flips wrap and line numbers through the handler', () => {
    const root = document.getElementById('c') as HTMLElement;
    addCodeActions([root]);
    let prefs: CodeBlockPrefs = { wrap: false, lineNumbers: false };
    const onToggle = vi.fn((next: CodeBlockPrefs) => (prefs = next));
    installCodeActions(root, () => prefs, onToggle);

    (root.querySelector('.code-action-wrap') as HTMLElement).click();
    expect(onToggle).toHaveBeenLastCalledWith({ wrap: true, lineNumbers: false });
    (root.querySelector('.code-action-lines') as HTMLElement).click();
    expect(onToggle).toHaveBeenLastCalledWith({ wrap: true, lineNumbers: true });
    (root.querySelector('.code-action-wrap') as HTMLElement).click();
    expect(onToggle).toHaveBeenLastCalledWith({ wrap: false, lineNumbers: true });
  });

  it('copies the code without the buttons or line numbers', async () => {
    const root = document.getElementById('c') as HTMLElement;
    addCodeActions([root]);
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    const onToggle = vi.fn();
    installCodeActions(root, () => ({ wrap: false, lineNumbers: true }), onToggle);

    (root.querySelector('.code-action-copy') as HTMLElement).click();
    expect(writeText).toHaveBeenCalledWith('a\nb\n');
    expect(onToggle).not.toHaveBeenCalled();
  });
});

describe('code-block prefs on <body>', () => {
  it('round-trips through the body classes', () => {
    document.body.className = '';
    expect(readStampedCodeBlockPrefs()).toEqual({ wrap: false, lineNumbers: false });
    applyCodeBlockPrefs({ wrap: true, lineNumbers: false });
    expect(document.body.classList.contains('code-wrap')).toBe(true);
    expect(document.body.classList.contains('code-line-numbers')).toBe(false);
    applyCodeBlockPrefs({ wrap: false, lineNumbers: true });
    expect(readStampedCodeBlockPrefs()).toEqual({ wrap: false, lineNumbers: true });
  });
});
