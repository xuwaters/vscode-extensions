import { describe, expect, it } from 'vitest';
import { baseName, joinPath, relativeTo } from './paths.js';

describe('baseName', () => {
  it('returns the last segment', () => {
    expect(baseName('/w/a/.cc-writes')).toBe('.cc-writes');
    expect(baseName('/w')).toBe('w');
    expect(baseName('/')).toBe('');
  });
});

describe('joinPath', () => {
  it('appends a child segment', () => {
    expect(joinPath('/w', '.gitignore')).toBe('/w/.gitignore');
    expect(joinPath('/', 'w')).toBe('/w');
  });
});

describe('relativeTo', () => {
  it('strips the base prefix', () => {
    expect(relativeTo('/w', '/w/a/b')).toBe('a/b');
    expect(relativeTo('/', '/w')).toBe('w');
    expect(relativeTo('/w', '/w')).toBe('');
  });

  it('returns undefined outside the base', () => {
    expect(relativeTo('/w', '/other/a')).toBeUndefined();
    // A shared prefix is not a shared directory.
    expect(relativeTo('/w', '/workspace/a')).toBeUndefined();
  });
});
