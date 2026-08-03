// Syntax highlighting for fenced code blocks. We use highlight.js's "common"
// bundle (≈40 popular languages) to keep the webview bundle reasonable while
// still covering the languages most markdown authors reach for.
import hljs from 'highlight.js/lib/common';
import capnp from './capnp';

// Languages the common bundle doesn't carry. Registered once at module load so
// their fences highlight exactly like the built-in ones.
hljs.registerLanguage('capnp', capnp);

/**
 * Highlight the engine-emitted `<pre><code class="language-…">` fences inside
 * `roots`. Math code fences (`data-math-style`) belong to KaTeX and plain
 * fences stay as escaped text.
 */
export function highlightCode(roots: Element[]): void {
  for (const root of roots) {
    for (const code of codeBlocksIn(root)) {
      const lang = languageOf(code);
      if (!lang || !hljs.getLanguage(lang)) continue;
      try {
        const result = hljs.highlight(code.textContent ?? '', {
          language: lang,
          ignoreIllegals: true,
        });
        code.innerHTML = result.value;
        code.classList.add('hljs');
        code.parentElement?.classList.add('hljs-pre');
      } catch {
        // Unsupported syntax for the grammar — leave the escaped text as-is.
      }
    }
  }
}

function codeBlocksIn(root: Element): HTMLElement[] {
  const selector = 'pre > code[class*="language-"]:not([data-math-style])';
  const out: HTMLElement[] = [];
  if (root.matches(selector)) out.push(root as HTMLElement);
  out.push(...root.querySelectorAll<HTMLElement>(selector));
  return out;
}

function languageOf(code: Element): string | null {
  for (const cls of code.classList) {
    if (cls.startsWith('language-')) {
      const lang = cls.slice('language-'.length).toLowerCase();
      return lang === 'math' ? null : lang;
    }
  }
  return null;
}
