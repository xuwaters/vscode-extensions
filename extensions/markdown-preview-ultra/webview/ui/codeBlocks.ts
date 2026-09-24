/**
 * Per-fence actions: soft wrap, line numbers, copy. The buttons are injected
 * into changed blocks after each patch; clicks are delegated from the content
 * root.
 *
 * Wrap and line numbers are not per-block: they are page-wide switches held by
 * the host for the window (see `CodeBlockStore`), and a page shows them as
 * `code-wrap` / `code-line-numbers` classes on <body>. So that flipping one is
 * only a class change, every fence is split into one `.code-line` span per
 * source line up front, and the numbers are a CSS counter over those spans.
 */
import type { CodeBlockPrefs } from '../../src/messages';

type Action = 'wrap' | 'lines' | 'copy';

const BUTTONS: { action: Action; label: string; title: string }[] = [
  { action: 'wrap', label: 'Wrap', title: 'Toggle soft wrap for code blocks' },
  { action: 'lines', label: 'Lines', title: 'Toggle line numbers for code blocks' },
  { action: 'copy', label: 'Copy', title: 'Copy code' },
];

export function addCodeActions(roots: Element[]): void {
  for (const root of roots) {
    const pres: Element[] = root.matches('pre') ? [root] : [];
    pres.push(...root.querySelectorAll('pre'));
    for (const pre of pres) {
      if (childOf(pre, 'div.code-actions')) continue;
      const code = childOf(pre, 'code');
      if (!code) continue;
      splitLines(code);
      const bar = document.createElement('div');
      bar.className = 'code-actions';
      for (const { action, label, title } of BUTTONS) {
        const button = document.createElement('button');
        button.className = `code-action code-action-${action}`;
        button.type = 'button';
        button.title = title;
        button.textContent = label;
        button.dataset.action = action;
        bar.append(button);
      }
      pre.append(bar);
    }
  }
}

/**
 * Rewrite `code`'s contents as one `.code-line` span per source line. A
 * highlighter's token span that runs across a line break is closed at the end
 * of one line and reopened at the start of the next, so each line stands on
 * its own. The `\n`s stay inside the lines: `textContent` — what Copy takes —
 * is unchanged.
 */
export function splitLines(code: HTMLElement): void {
  if (childOf(code, 'span.code-line')) return;
  const lines: HTMLElement[] = [];
  /** The source elements enclosing the node being walked, outermost first. */
  const open: Element[] = [];
  let cursor: Element = code;

  const startLine = (): void => {
    const line = document.createElement('span');
    line.className = 'code-line';
    lines.push(line);
    cursor = line;
    for (const el of open) {
      const clone = el.cloneNode(false) as Element;
      cursor.append(clone);
      cursor = clone;
    }
  };

  const walk = (nodes: Node[]): void => {
    for (const node of nodes) {
      if (node.nodeType === Node.TEXT_NODE) {
        const parts = (node.nodeValue ?? '').split('\n');
        parts.forEach((part, i) => {
          if (i > 0) {
            cursor.append('\n');
            startLine();
          }
          if (part) cursor.append(part);
        });
      } else if (node instanceof Element) {
        const clone = node.cloneNode(false) as Element;
        cursor.append(clone);
        open.push(node);
        cursor = clone;
        walk(Array.from(node.childNodes));
        open.pop();
        // The cursor is on the current line's copy of `node` — `clone` itself,
        // or a later line's copy if `node` spanned a line break. Its parent is
        // that line's copy of whatever encloses `node`.
        cursor = cursor.parentElement as Element;
      }
    }
  };

  const nodes = Array.from(code.childNodes);
  if (nodes.length === 0) return;
  startLine();
  walk(nodes);
  // The fence's closing newline leaves an empty last line behind.
  const last = lines[lines.length - 1];
  if (lines.length > 1 && last.textContent === '') lines.pop();
  code.replaceChildren(...lines);
  // Wide enough for the longest number, so the gutter doesn't jitter per block.
  code.style.setProperty('--line-digits', String(String(lines.length).length));
}

/**
 * Wire the delegated click handler (once, on the content root). Wrap and line
 * numbers are handed to `onToggle` with the switch they flip; Copy is local.
 */
export function installCodeActions(
  content: HTMLElement,
  current: () => CodeBlockPrefs,
  onToggle: (next: CodeBlockPrefs) => void,
): void {
  content.addEventListener('click', (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>(
      'button.code-action',
    );
    if (!button) return;
    const prefs = current();
    switch (button.dataset.action as Action) {
      case 'wrap':
        onToggle({ ...prefs, wrap: !prefs.wrap });
        break;
      case 'lines':
        onToggle({ ...prefs, lineNumbers: !prefs.lineNumbers });
        break;
      case 'copy':
        copy(button);
        break;
    }
  });
}

function copy(button: HTMLButtonElement): void {
  const pre = button.closest('pre');
  const code = pre ? childOf(pre, 'code') : null;
  const text = code?.textContent ?? '';
  void navigator.clipboard.writeText(text).then(() => {
    button.textContent = 'Copied';
    button.classList.add('copied');
    setTimeout(() => {
      button.textContent = 'Copy';
      button.classList.remove('copied');
    }, 1200);
  });
}

function childOf(parent: Element, selector: string): HTMLElement | null {
  for (const child of parent.children) {
    if (child.matches(selector)) return child as HTMLElement;
  }
  return null;
}

/** Show the switches on <body>; the stylesheet does the rest, buttons included. */
export function applyCodeBlockPrefs(prefs: CodeBlockPrefs): void {
  const cls = document.body.classList;
  cls.toggle('code-wrap', prefs.wrap);
  cls.toggle('code-line-numbers', prefs.lineNumbers);
}

/** The switches as the host stamped them onto <body>. */
export function readStampedCodeBlockPrefs(): CodeBlockPrefs {
  const cls = document.body.classList;
  return {
    wrap: cls.contains('code-wrap'),
    lineNumbers: cls.contains('code-line-numbers'),
  };
}
