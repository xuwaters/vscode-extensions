import './styles/preview.css';
import './styles/frontmatter.css';
import './styles/highlight.css';
import './styles/math.css';
import './styles/mermaid.css';
import 'katex/dist/katex.min.css';

import { MarkdownRenderer } from './markdown';
import { renderMermaid } from './mermaid';
import type {
  HostToWebview,
  UpdateMessage,
  WebviewToHost,
} from '../src/messages';

declare function acquireVsCodeApi(): {
  postMessage(message: WebviewToHost): void;
  getState(): unknown;
  setState(state: unknown): void;
};

const vscode = acquireVsCodeApi();
const content = document.getElementById('content') as HTMLElement;
const renderer = new MarkdownRenderer();

function currentTheme(): 'light' | 'dark' {
  const cls = document.body.classList;
  return cls.contains('vscode-light') || cls.contains('vscode-high-contrast-light')
    ? 'light'
    : 'dark';
}

window.addEventListener('message', (event) => {
  const msg = event.data as HostToWebview;
  switch (msg.type) {
    case 'update':
      handleUpdate(msg);
      break;
    case 'scroll':
      scrollToLine(msg.line);
      break;
  }
});

function handleUpdate(msg: UpdateMessage): void {
  // Keep the reader's place while editing: remember the top source line,
  // re-render, then restore to the same line after layout settles.
  const topLine = currentTopLine();
  content.innerHTML = renderer.render(msg.markdown, msg.baseHref, msg.settings);

  if (msg.settings.mermaid) {
    void renderMermaid(content, currentTheme());
  }

  requestAnimationFrame(() => scrollToLine(topLine));
}

// ── Scroll sync ──────────────────────────────────────────────────────

/** Absolute document offset of an element's top edge. */
function absoluteTop(el: HTMLElement): number {
  return el.getBoundingClientRect().top + window.scrollY;
}

/** The source line of the block currently at the top of the viewport. */
function currentTopLine(): number {
  const els = content.querySelectorAll<HTMLElement>('[data-line]');
  for (const el of els) {
    const rect = el.getBoundingClientRect();
    if (rect.bottom >= 0) return Number(el.getAttribute('data-line'));
  }
  return 0;
}

/** Scroll the preview so that source `line` sits near the top of the view. */
function scrollToLine(line: number): void {
  const els = Array.from(content.querySelectorAll<HTMLElement>('[data-line]'));
  if (els.length === 0) return;

  let before = els[0];
  let after: HTMLElement | null = null;
  for (const el of els) {
    const l = Number(el.getAttribute('data-line'));
    if (l <= line) {
      before = el;
    } else {
      after = el;
      break;
    }
  }

  const beforeLine = Number(before.getAttribute('data-line'));
  let top = absoluteTop(before);
  if (after) {
    const afterLine = Number(after.getAttribute('data-line'));
    if (afterLine > beforeLine) {
      const progress = (line - beforeLine) / (afterLine - beforeLine);
      top += progress * (absoluteTop(after) - top);
    }
  }

  window.scrollTo({ top: Math.max(0, top - 8), behavior: 'auto' });
}

// ── Link handling ────────────────────────────────────────────────────

content.addEventListener('click', (event) => {
  const anchor = (event.target as HTMLElement).closest('a');
  if (!anchor) return;
  const href = anchor.getAttribute('href');
  if (!href) return;

  event.preventDefault();
  if (href.startsWith('#')) {
    const target = document.getElementById(href.slice(1));
    target?.scrollIntoView({ behavior: 'smooth' });
    return;
  }
  vscode.postMessage({ type: 'openLink', href });
});

// Signal readiness; the host replies with the first `update`.
vscode.postMessage({ type: 'ready' });
