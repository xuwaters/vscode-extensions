import { describe, expect, it } from 'vitest';
import { globMatch, IgnoreStack, parseIgnore } from './gitignore.js';

const m = globMatch;

describe('globMatch', () => {
  it('matches literals and single stars', () => {
    expect(m('target', 'target')).toBe(true);
    expect(m('targets', 'target')).toBe(false);
    expect(m('main.rs', '*.rs')).toBe(true);
    expect(m('src/main.rs', '*.rs')).toBe(false);
    expect(m('src/main.rs', 'src/*.rs')).toBe(true);
    expect(m('a.txt', '?.txt')).toBe(true);
    expect(m('ab.txt', '?.txt')).toBe(false);
  });

  it('crosses slashes only for a double star', () => {
    expect(m('a/b/c.rs', '**/c.rs')).toBe(true);
    expect(m('c.rs', '**/c.rs')).toBe(true);
    expect(m('a/b/c.rs', 'a/**/c.rs')).toBe(true);
    expect(m('a/c.rs', 'a/**/c.rs')).toBe(true);
    expect(m('a/b/c.rs', 'a/**')).toBe(true);
  });

  it('matches charsets', () => {
    expect(m('a1', 'a[0-9]')).toBe(true);
    expect(m('ax', 'a[0-9]')).toBe(false);
    expect(m('ax', 'a[^0-9]')).toBe(true);
  });

  it('matches an escaped metacharacter literally', () => {
    expect(m('a*b', 'a\\*b')).toBe(true);
    expect(m('axb', 'a\\*b')).toBe(false);
  });
});

describe('parseIgnore', () => {
  it('skips blanks and comments', () => {
    expect(parseIgnore('\n# comment\n  \ntarget\n')).toEqual([
      { pattern: 'target', negated: false, dirOnly: false, anchored: false },
    ]);
  });

  it('reads negation, directory-only and anchored markers', () => {
    expect(parseIgnore('!build/\n/root-only\nsrc/lib\n')).toEqual([
      { pattern: 'build', negated: true, dirOnly: true, anchored: false },
      { pattern: 'root-only', negated: false, dirOnly: false, anchored: true },
      { pattern: 'src/lib', negated: false, dirOnly: false, anchored: true },
    ]);
  });

  it('drops lines that carry no pattern', () => {
    expect(parseIgnore('!\n/\n!/\n')).toEqual([]);
  });
});

/** A stack standing in for files found along a traversal, shallowest first. */
function stack(files: [string, string][]): IgnoreStack {
  const st = new IgnoreStack();
  for (const [base, content] of files) st.pushRules(base, content);
  return st;
}

describe('IgnoreStack', () => {
  it('applies rules to every level below the file', () => {
    const st = stack([['/w', 'target\nnode_modules\n*.log\n']]);
    expect(st.isIgnored('/w/target', true)).toBe(true);
    expect(st.isIgnored('/w/a/b/target', true)).toBe(true);
    expect(st.isIgnored('/w/a/x.log', false)).toBe(true);
    expect(st.isIgnored('/w/src', true)).toBe(false);
  });

  it('honours directory-only and anchored rules', () => {
    const st = stack([['/w', 'build/\n/root-only\n']]);
    expect(st.isIgnored('/w/build', true)).toBe(true);
    expect(st.isIgnored('/w/build', false)).toBe(false);
    expect(st.isIgnored('/w/root-only', true)).toBe(true);
    expect(st.isIgnored('/w/sub/root-only', true)).toBe(false);
  });

  it('lets the last matching rule and the deepest file win', () => {
    let st = stack([['/w', '*.log\n!keep.log\n']]);
    expect(st.isIgnored('/w/a.log', false)).toBe(true);
    expect(st.isIgnored('/w/keep.log', false)).toBe(false);

    st = stack([
      ['/w', '*.log\n'],
      ['/w/sub', '!*.log\n'],
    ]);
    expect(st.isIgnored('/w/a.log', false)).toBe(true);
    expect(st.isIgnored('/w/sub/a.log', false)).toBe(false);
  });

  it('ignores nothing once the files are popped', () => {
    const st = stack([['/w', 'target\n']]);
    st.pop();
    expect(st.isIgnored('/w/target', true)).toBe(false);
  });

  it('ignores paths outside the file base and the base itself', () => {
    const st = stack([['/w', 'w\ntarget\n']]);
    expect(st.isIgnored('/w', true)).toBe(false);
    expect(st.isIgnored('/other/target', true)).toBe(false);
  });
});
