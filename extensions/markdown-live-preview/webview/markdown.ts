import MarkdownIt from 'markdown-it';
import markdownItFootnote from 'markdown-it-footnote';
import markdownItFrontMatter from 'markdown-it-front-matter';
import markdownItTaskLists from 'markdown-it-task-lists';
import markdownItTexmath from 'markdown-it-texmath';
import katex from 'katex';
import yaml from 'js-yaml';
import { highlightToHtml } from './highlight';
import type { PreviewSettings } from '../src/messages';

/** Exact type of a markdown-it renderer rule (avoids `any` on the wrappers). */
type RenderRule = NonNullable<MarkdownIt['renderer']['rules'][string]>;

/** Block-level tokens that get a `data-line` anchor for editor scroll sync. */
const LINE_MAPPED_TOKENS = [
  'paragraph_open',
  'heading_open',
  'blockquote_open',
  'bullet_list_open',
  'ordered_list_open',
  'list_item_open',
  'table_open',
  'hr',
];

/**
 * Wraps a markdown-it instance and rebuilds it whenever render settings
 * change. Rendering is otherwise stateless: the host owns the document and
 * ships its full text on every update.
 */
export class MarkdownRenderer {
  private md: MarkdownIt | null = null;
  private signature = '';
  private settings: PreviewSettings | null = null;
  private baseHref = '';
  private frontmatter: string | null = null;

  /** Render `markdown` to HTML, resolving relative images against `baseHref`. */
  render(markdown: string, baseHref: string, settings: PreviewSettings): string {
    this.ensure(settings);
    this.baseHref = baseHref;
    this.frontmatter = null;

    const body = this.md!.render(markdown);
    const card =
      settings.frontmatter && this.frontmatter !== null
        ? renderFrontmatterCard(this.frontmatter)
        : '';
    return card + body;
  }

  private ensure(settings: PreviewSettings): void {
    const signature = JSON.stringify(settings);
    if (this.md && signature === this.signature) return;
    this.signature = signature;
    this.settings = settings;
    this.md = this.build(settings);
  }

  private build(settings: PreviewSettings): MarkdownIt {
    const md = new MarkdownIt({
      html: true,
      linkify: settings.linkify,
      breaks: settings.breaks,
      typographer: false,
    });

    md.use(markdownItTaskLists, { label: true });
    md.use(markdownItFootnote);

    // Always strip leading YAML so it never renders as a stray `<hr>` + text;
    // the captured text is shown as a card by `render()` when enabled.
    md.use(markdownItFrontMatter, (fm: string) => {
      this.frontmatter = fm;
    });

    if (settings.math) {
      md.use(markdownItTexmath, {
        engine: katex,
        delimiters: 'dollars',
        katexOptions: { throwOnError: false, output: 'htmlAndMathml' },
      });
    }

    this.installLineNumbers(md);
    this.installImageResolver(md);
    this.installFence(md);

    return md;
  }

  /** Tag block elements with `data-line` so the host can scroll-sync. */
  private installLineNumbers(md: MarkdownIt): void {
    const renderToken: RenderRule = (tokens, idx, options, _env, self) =>
      self.renderToken(tokens, idx, options);

    for (const name of LINE_MAPPED_TOKENS) {
      const original = md.renderer.rules[name] ?? renderToken;
      md.renderer.rules[name] = (tokens, idx, options, env, self) => {
        const token = tokens[idx];
        if (token.map) {
          token.attrSet('data-line', String(token.map[0]));
          token.attrJoin('class', 'code-line');
        }
        return original(tokens, idx, options, env, self);
      };
    }
  }

  /** Rewrite relative `<img src>` to webview-resource URIs. */
  private installImageResolver(md: MarkdownIt): void {
    const renderToken: RenderRule = (tokens, idx, options, _env, self) =>
      self.renderToken(tokens, idx, options);
    const original = md.renderer.rules.image ?? renderToken;

    md.renderer.rules.image = (tokens, idx, options, env, self) => {
      const token = tokens[idx];
      const srcIndex = token.attrIndex('src');
      if (srcIndex >= 0 && token.attrs) {
        token.attrs[srcIndex][1] = this.resolveSrc(token.attrs[srcIndex][1]);
      }
      return original(tokens, idx, options, env, self);
    };
  }

  /** Custom fence: mermaid containers + highlighted code, both line-mapped. */
  private installFence(md: MarkdownIt): void {
    md.renderer.rules.fence = (tokens, idx) => {
      const token = tokens[idx];
      const info = token.info.trim();
      const lang = info.split(/\s+/g)[0] ?? '';
      const line = token.map ? token.map[0] : 0;

      if (this.settings?.mermaid && lang.toLowerCase() === 'mermaid') {
        return `<div class="mermaid-container code-line" data-line="${line}" data-mermaid-source="${encodeURIComponent(
          token.content,
        )}"></div>`;
      }

      const code = highlightToHtml(token.content, lang);
      const langClass = lang ? ` class="language-${escapeAttr(lang)}"` : '';
      return `<pre class="hljs code-line" data-line="${line}"><code${langClass}>${code}</code></pre>`;
    };
  }

  private resolveSrc(src: string): string {
    if (!src || !this.baseHref) return src;
    if (/^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(src)) return src;
    return this.baseHref + src.replace(/^\.\//, '').replace(/ /g, '%20');
  }
}

/** Render captured frontmatter YAML as a flat metadata card. */
function renderFrontmatterCard(raw: string): string {
  let data: unknown = null;
  try {
    data = yaml.load(raw);
  } catch {
    data = null;
  }

  if (!data || typeof data !== 'object' || Array.isArray(data)) {
    return `<div class="frontmatter-card"><div class="frontmatter-label">Frontmatter</div><pre>${escapeHtml(
      raw.trim(),
    )}</pre></div>`;
  }

  const rows = Object.entries(data as Record<string, unknown>)
    .map(
      ([key, value]) =>
        `<dt>${escapeHtml(key)}</dt><dd>${escapeHtml(formatValue(value))}</dd>`,
    )
    .join('');
  return `<div class="frontmatter-card"><div class="frontmatter-label">Frontmatter</div><dl>${rows}</dl></div>`;
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (Array.isArray(value)) return value.map((v) => formatValue(v)).join(', ');
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function escapeAttr(text: string): string {
  return text.replace(/[^a-zA-Z0-9_-]/g, '');
}
