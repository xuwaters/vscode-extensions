import { describe, expect, it } from 'vitest';
import { MarkdownRenderer } from './markdown';
import type { PreviewSettings } from '../src/messages';

function settings(over: Partial<PreviewSettings> = {}): PreviewSettings {
  return {
    math: true,
    mermaid: true,
    frontmatter: true,
    breaks: false,
    linkify: true,
    scrollSync: true,
    ...over,
  };
}

describe('MarkdownRenderer', () => {
  const renderer = new MarkdownRenderer();

  it('tags block elements with data-line for scroll sync', () => {
    const html = renderer.render('# Title\n\nfirst\n\nsecond', '', settings());
    expect(html).toContain('<h1');
    expect(html).toContain('data-line="0"');
    expect(html).toContain('data-line="2"');
  });

  it('syntax-highlights fenced code blocks', () => {
    const html = renderer.render('```js\nconst x = 1;\n```', '', settings());
    expect(html).toContain('class="hljs code-line"');
    expect(html).toContain('hljs-keyword'); // `const` got tokenized
  });

  it('emits a mermaid container when mermaid is enabled', () => {
    const html = renderer.render('```mermaid\ngraph TD;A-->B;\n```', '', settings());
    expect(html).toContain('mermaid-container');
    expect(html).toContain('data-mermaid-source=');
  });

  it('falls back to a code block when mermaid is disabled', () => {
    const html = renderer.render(
      '```mermaid\ngraph TD;A-->B;\n```',
      '',
      settings({ mermaid: false }),
    );
    expect(html).not.toContain('mermaid-container');
    expect(html).toContain('hljs');
  });

  it('renders KaTeX math when enabled and leaves it literal when disabled', () => {
    expect(renderer.render('$E=mc^2$', '', settings())).toContain('katex');
    const off = renderer.render('$E=mc^2$', '', settings({ math: false }));
    expect(off).not.toContain('katex');
    expect(off).toContain('$E=mc^2$');
  });

  it('renders frontmatter as a card instead of a horizontal rule', () => {
    const html = renderer.render(
      '---\ntitle: Hello\ntags: [a, b]\n---\n\nbody',
      '',
      settings(),
    );
    expect(html).toContain('frontmatter-card');
    expect(html).toContain('Hello');
    expect(html).toContain('a, b');
    expect(html).not.toContain('<hr');
  });

  it('resolves relative image sources against the base href', () => {
    const html = renderer.render('![cat](img/cat.png)', 'https://host/root/', settings());
    expect(html).toContain('src="https://host/root/img/cat.png"');
  });

  it('leaves absolute and data image sources untouched', () => {
    const html = renderer.render(
      '![a](https://e.com/a.png)',
      'https://host/root/',
      settings(),
    );
    expect(html).toContain('src="https://e.com/a.png"');
  });
});
