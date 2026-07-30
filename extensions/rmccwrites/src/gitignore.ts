//! Minimal `.gitignore` support: a stack of per-directory rule sets plus a glob
//! matcher with git's pattern semantics (`*`, `**`, `?`, `[charset]`, `!`
//! negation, trailing `/` for directory-only, leading `/` for anchoring).

import { baseName, joinPath, relativeTo } from './paths.js';
import type { Vfs } from './vfs.js';

export interface Rule {
  pattern: string;
  negated: boolean;
  dirOnly: boolean;
  /**
   * Pattern contains a `/` other than a trailing one, so it is matched against
   * the whole path relative to the file's directory instead of the base name.
   */
  anchored: boolean;
}

interface IgnoreFile {
  /** Directory the file lives in; patterns are relative to it. */
  base: string;
  rules: Rule[];
}

/**
 * A stack of `.gitignore` files, ordered shallowest first, mirroring the
 * directories currently on the traversal path.
 */
export class IgnoreStack {
  private readonly files: IgnoreFile[] = [];

  /**
   * Read `dir/.gitignore`, if present, and push it onto the stack. Always pairs
   * with exactly one {@link IgnoreStack.pop}.
   */
  async push(dir: string, fs: Vfs): Promise<void> {
    let content = '';
    try {
      content = await fs.readText(joinPath(dir, '.gitignore'));
    } catch {
      // No readable .gitignore here; a no-op entry keeps push/pop balanced.
    }
    this.pushRules(dir, content);
  }

  /** Push the rules parsed from `content` as if they came from `dir`. */
  pushRules(dir: string, content: string): void {
    this.files.push({ base: dir, rules: parseIgnore(content) });
  }

  pop(): void {
    this.files.pop();
  }

  /**
   * Whether `path` is ignored. Deeper `.gitignore` files override shallower
   * ones; within one file the last matching rule wins.
   */
  isIgnored(path: string, isDir: boolean): boolean {
    let ignored = false;
    for (const file of this.files) {
      const rel = relativeTo(file.base, path);
      if (rel === undefined || rel === '') continue;
      const name = baseName(rel);
      for (const rule of file.rules) {
        if (rule.dirOnly && !isDir) continue;
        const subject = rule.anchored ? rel : name;
        if (globMatch(subject, rule.pattern)) ignored = !rule.negated;
      }
    }
    return ignored;
  }
}

export function parseIgnore(content: string): Rule[] {
  const rules: Rule[] = [];
  for (const rawLine of content.split('\n')) {
    // Leading whitespace is significant to git only when escaped; trailing
    // whitespace is stripped unless escaped. Keep it simple: trim both.
    const line = rawLine.trim();
    if (line === '' || line.startsWith('#')) continue;

    let s = line;
    const negated = s.startsWith('!');
    if (negated) s = s.slice(1);
    const dirOnly = s.endsWith('/');
    if (dirOnly) s = s.slice(0, -1);
    if (s === '') continue;

    const anchored = s.includes('/');
    const pattern = s.startsWith('/') ? s.slice(1) : s;
    if (pattern === '') continue;

    rules.push({ pattern, negated, dirOnly, anchored });
  }
  return rules;
}

/** Glob match with git semantics: `*` and `?` never cross `/`, `**` does. */
export function globMatch(name: string, pat: string): boolean {
  let ni = 0;
  let pi = 0;

  while (pi < pat.length) {
    const c = pat[pi];

    if (c === '*' && pat[pi + 1] === '*') {
      let rest = pat.slice(pi + 2);
      if (rest === '') return true;
      // `**/x` matches `x` at any depth, including depth zero.
      if (rest.startsWith('/')) {
        rest = rest.slice(1);
        if (globMatch(name.slice(ni), rest)) return true;
      }
      for (let i = ni; i <= name.length; i++) {
        if (i > ni && name[i - 1] !== '/') continue;
        if (globMatch(name.slice(i), rest)) return true;
      }
      return false;
    }

    if (c === '*') {
      const rest = pat.slice(pi + 1);
      if (rest === '') return !name.slice(ni).includes('/');
      for (let i = ni; i <= name.length; i++) {
        if (i > ni && name[i - 1] === '/') break;
        if (globMatch(name.slice(i), rest)) return true;
      }
      return false;
    }

    if (c === '?') {
      if (ni >= name.length || name[ni] === '/') return false;
      ni++;
      pi++;
      continue;
    }

    if (c === '[') {
      if (ni >= name.length || name[ni] === '/') return false;
      pi++;
      const negate = pat[pi] === '^' || pat[pi] === '!';
      if (negate) pi++;
      let matched = false;
      while (pi < pat.length && pat[pi] !== ']') {
        let lo = pat[pi];
        if (lo === '\\' && pi + 1 < pat.length) {
          pi++;
          lo = pat[pi];
        }
        if (pi + 2 < pat.length && pat[pi + 1] === '-' && pat[pi + 2] !== ']') {
          const hi = pat[pi + 2];
          if (name[ni] >= lo && name[ni] <= hi) matched = true;
          pi += 3;
        } else {
          if (name[ni] === lo) matched = true;
          pi++;
        }
      }
      if (pi < pat.length) pi++; // skip ']'
      if (matched === negate) return false;
      ni++;
      continue;
    }

    if (c === '\\' && pi + 1 < pat.length) {
      pi++;
      if (ni >= name.length || name[ni] !== pat[pi]) return false;
      ni++;
      pi++;
      continue;
    }

    if (ni >= name.length || name[ni] !== c) return false;
    ni++;
    pi++;
  }

  return ni >= name.length;
}
