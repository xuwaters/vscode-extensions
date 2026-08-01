import type { Patch } from '../src/messages';

/**
 * Apply the engine's patch script to the preview's flat list of top-level
 * block elements. A cursor walks the current children: `keep` advances,
 * `replace`/`insert` parse HTML strings into elements, `delete` removes.
 *
 * Untouched blocks keep their DOM nodes, so images, open `<details>`,
 * selection, and rendered mermaid SVGs survive edits elsewhere.
 *
 * Returns the inserted/replaced elements — the only ones that need
 * post-processing. Throws if the script doesn't fit the current DOM (the
 * caller reports the error; the host answers with a `reset` render).
 */
export function applyPatches(container: HTMLElement, patches: Patch[]): Element[] {
  const changed: Element[] = [];
  let cursor = container.firstElementChild;

  const advance = (count: number): Element => {
    let last: Element | null = null;
    for (let i = 0; i < count; i++) {
      if (!cursor) throw new Error('patch cursor ran past end of block list');
      last = cursor;
      cursor = cursor.nextElementSibling;
    }
    if (!last) throw new Error('patch op with zero span');
    return last;
  };

  for (const patch of patches) {
    switch (patch.op) {
      case 'keep':
        advance(patch.count);
        break;
      case 'delete': {
        for (let i = 0; i < patch.count; i++) {
          if (!cursor) throw new Error('delete ran past end of block list');
          const next: Element | null = cursor.nextElementSibling;
          cursor.remove();
          cursor = next;
        }
        break;
      }
      case 'insert': {
        for (const el of parseBlocks(patch.html)) {
          container.insertBefore(el, cursor);
          changed.push(el);
        }
        break;
      }
      case 'replace': {
        const els = parseBlocks(patch.html);
        for (const el of els) {
          container.insertBefore(el, cursor);
          changed.push(el);
        }
        for (let i = 0; i < patch.count; i++) {
          if (!cursor) throw new Error('replace ran past end of block list');
          const next: Element | null = cursor.nextElementSibling;
          cursor.remove();
          cursor = next;
        }
        break;
      }
    }
  }
  return changed;
}

/**
 * Parse block HTML strings into elements. Each engine block is one top-level
 * element; a multi-element parse is wrapped so the block list stays 1:1 with
 * the engine's.
 */
function parseBlocks(htmlBlocks: string[]): Element[] {
  const out: Element[] = [];
  for (const html of htmlBlocks) {
    const template = document.createElement('template');
    template.innerHTML = html;
    const els = Array.from(template.content.children);
    if (els.length === 1) {
      out.push(els[0]);
    } else {
      // Raw-HTML blocks can parse to several (or zero) elements; keep the
      // 1-block : 1-element invariant with a neutral wrapper.
      const wrapper = document.createElement('div');
      wrapper.className = 'block-group';
      wrapper.append(...template.content.childNodes);
      out.push(wrapper);
    }
  }
  return out;
}
