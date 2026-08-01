// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest';
import type { Patch } from '../src/messages';
import { applyPatches } from './patch';

let container: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = '<div id="c"></div>';
  container = document.getElementById('c') as HTMLElement;
});

function blocks(): string[] {
  return Array.from(container.children).map((el) => el.outerHTML);
}

function seed(...html: string[]): void {
  applyPatches(container, [{ op: 'insert', html }]);
}

describe('applyPatches', () => {
  it('inserts into an empty container', () => {
    const changed = applyPatches(container, [
      { op: 'insert', html: ['<p>a</p>', '<p>b</p>'] },
    ]);
    expect(blocks()).toEqual(['<p>a</p>', '<p>b</p>']);
    expect(changed).toHaveLength(2);
  });

  it('replaces only the targeted block', () => {
    seed('<p>a</p>', '<p>b</p>', '<p>c</p>');
    const keep = container.children[2];
    const changed = applyPatches(container, [
      { op: 'keep', count: 1 },
      { op: 'replace', count: 1, html: ['<p>B</p>'] },
      { op: 'keep', count: 1 },
    ]);
    expect(blocks()).toEqual(['<p>a</p>', '<p>B</p>', '<p>c</p>']);
    expect(changed).toHaveLength(1);
    // Untouched blocks keep their identity (DOM state survives).
    expect(container.children[2]).toBe(keep);
  });

  it('deletes and inserts in multiple regions', () => {
    seed('<p>a</p>', '<p>b</p>', '<p>c</p>', '<p>d</p>');
    applyPatches(container, [
      { op: 'delete', count: 1 },
      { op: 'keep', count: 1 },
      { op: 'insert', html: ['<p>x</p>'] },
      { op: 'keep', count: 2 },
    ]);
    expect(blocks()).toEqual(['<p>b</p>', '<p>x</p>', '<p>c</p>', '<p>d</p>']);
  });

  it('appends via trailing insert', () => {
    seed('<p>a</p>');
    applyPatches(container, [
      { op: 'keep', count: 1 },
      { op: 'insert', html: ['<p>z</p>'] },
    ]);
    expect(blocks()).toEqual(['<p>a</p>', '<p>z</p>']);
  });

  it('wraps multi-element raw-HTML blocks to preserve the 1:1 invariant', () => {
    applyPatches(container, [{ op: 'insert', html: ['<p>a</p><p>b</p>'] }]);
    expect(container.children).toHaveLength(1);
    expect(container.children[0].className).toBe('block-group');
  });

  it('throws when the script overruns the DOM', () => {
    seed('<p>a</p>');
    const bad: Patch[] = [{ op: 'keep', count: 2 }];
    expect(() => applyPatches(container, bad)).toThrow(/past end/);
  });

  it('throws on delete past the end', () => {
    seed('<p>a</p>');
    expect(() =>
      applyPatches(container, [{ op: 'delete', count: 2 }]),
    ).toThrow(/past end/);
  });
});
