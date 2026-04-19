import { describe, expect, it } from 'vitest';
import { appendGitignore, buildGitignore } from './generate.js';
import { getTemplate, TEMPLATES } from './templates.js';

describe('buildGitignore', () => {
  it('returns empty string for no templates', () => {
    expect(buildGitignore([])).toBe('');
  });

  it('emits a section marker with the template label', () => {
    const node = getTemplate('node')!;
    const output = buildGitignore([node]);
    expect(output).toContain('### Gitignore Generator: Node');
    expect(output).toContain('node_modules/');
  });

  it('concatenates multiple templates with a blank line between sections', () => {
    const node = getTemplate('node')!;
    const macos = getTemplate('macos')!;
    const output = buildGitignore([node, macos]);
    expect(output).toContain('### Gitignore Generator: Node');
    expect(output).toContain('### Gitignore Generator: macOS');
    // Two sections separated by blank line
    const marker = output.indexOf('### Gitignore Generator: macOS');
    expect(output.slice(marker - 2, marker)).toBe('\n\n');
  });

  it('all bundled templates have non-empty content', () => {
    for (const t of TEMPLATES) {
      expect(t.content.length).toBeGreaterThan(0);
    }
  });
});

describe('appendGitignore', () => {
  it('returns existing content unchanged when no templates given', () => {
    expect(appendGitignore('foo\n', [])).toBe('foo\n');
  });

  it('returns the built gitignore when existing is empty', () => {
    const rust = getTemplate('rust')!;
    const output = appendGitignore('', [rust]);
    expect(output).toBe(buildGitignore([rust]));
  });

  it('returns the built gitignore when existing is only whitespace', () => {
    const rust = getTemplate('rust')!;
    const output = appendGitignore('  \n\n', [rust]);
    expect(output).toBe(buildGitignore([rust]));
  });

  it('appends with a blank line between existing content and new sections', () => {
    const rust = getTemplate('rust')!;
    const output = appendGitignore('existing\nlines\n', [rust]);
    expect(output.startsWith('existing\nlines\n\n### Gitignore Generator: Rust')).toBe(true);
  });

  it('collapses trailing whitespace on existing content', () => {
    const rust = getTemplate('rust')!;
    const output = appendGitignore('existing\n\n\n', [rust]);
    expect(output.startsWith('existing\n\n### Gitignore Generator: Rust')).toBe(true);
  });
});
