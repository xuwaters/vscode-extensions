import * as path from 'path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { systemFontCandidates, systemFontDirs } from './fonts.js';
import { defaultCacheDir } from './packages.js';

/**
 * P4-09, in part.
 *
 * Verifying the extension on Windows and Linux needs those machines, and this
 * suite is not a substitute for running it there. What it *can* do is exercise
 * every platform branch from any host, so a Windows-only path bug is a failing
 * test rather than a bug report — which is the half of P4-09 that is reachable
 * from CI.
 *
 * The remaining half — that a forked Node process, real font files, and a real
 * package cache behave on those platforms — is still open. See the task.
 */

/** Run a body as if on another platform, with a chosen environment. */
function onPlatform<T>(
  platform: NodeJS.Platform,
  env: Record<string, string | undefined>,
  body: () => T,
): T {
  const originalPlatform = Object.getOwnPropertyDescriptor(process, 'platform');
  const originalEnv = { ...process.env };

  Object.defineProperty(process, 'platform', { value: platform, configurable: true });
  for (const [key, value] of Object.entries(env)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }

  try {
    return body();
  } finally {
    if (originalPlatform) Object.defineProperty(process, 'platform', originalPlatform);
    process.env = originalEnv;
  }
}

describe('the package cache directory, per platform', () => {
  afterEach(() => vi.restoreAllMocks());

  it('follows the XDG cache directory on Linux', () => {
    const dir = onPlatform(
      'linux',
      { HOME: '/home/u', XDG_CACHE_HOME: undefined, USERPROFILE: undefined },
      defaultCacheDir,
    );
    expect(dir).toBe(path.join('/home/u', '.cache', 'typst', 'packages'));
  });

  it('honours XDG_CACHE_HOME when it is set', () => {
    const dir = onPlatform(
      'linux',
      { HOME: '/home/u', XDG_CACHE_HOME: '/var/cache/u' },
      defaultCacheDir,
    );
    expect(dir).toBe(path.join('/var/cache/u', 'typst', 'packages'));
  });

  it('uses ~/Library/Caches on macOS', () => {
    const dir = onPlatform('darwin', { HOME: '/Users/u' }, defaultCacheDir);
    expect(dir).toBe(path.join('/Users/u', 'Library', 'Caches', 'typst', 'packages'));
  });

  it('uses LOCALAPPDATA on Windows', () => {
    const dir = onPlatform(
      'win32',
      { USERPROFILE: 'C:\\Users\\u', LOCALAPPDATA: 'C:\\Users\\u\\AppData\\Local' },
      defaultCacheDir,
    );
    expect(dir).toBe(
      path.join('C:\\Users\\u\\AppData\\Local', 'typst', 'packages'),
    );
  });

  it('falls back to a path under the profile when LOCALAPPDATA is missing', () => {
    const dir = onPlatform(
      'win32',
      { USERPROFILE: 'C:\\Users\\u', LOCALAPPDATA: undefined, HOME: undefined },
      defaultCacheDir,
    );
    expect(dir).toContain(path.join('AppData', 'Local'));
  });

  it('always ends at typst/packages, which is what typst-cli shares', () => {
    for (const platform of ['linux', 'darwin', 'win32'] as NodeJS.Platform[]) {
      const dir = onPlatform(platform, { HOME: '/h', USERPROFILE: 'C:\\u' }, defaultCacheDir);
      expect(dir.endsWith(path.join('typst', 'packages'))).toBe(true);
    }
  });
});

describe('system font directories, per platform', () => {
  const candidates = (platform: NodeJS.Platform, env: NodeJS.ProcessEnv) =>
    systemFontCandidates(platform, env);

  it('looks in the three places macOS keeps fonts', () => {
    const dirs = candidates('darwin', { HOME: '/Users/u' });
    expect(dirs).toContain('/System/Library/Fonts');
    expect(dirs).toContain('/Library/Fonts');
    expect(dirs).toContain(path.join('/Users/u', 'Library/Fonts'));
  });

  it('looks in the system and per-user directories on Linux', () => {
    const dirs = candidates('linux', { HOME: '/home/u' });
    expect(dirs).toContain('/usr/share/fonts');
    expect(dirs).toContain('/usr/local/share/fonts');
    expect(dirs).toContain(path.join('/home/u', '.local/share/fonts'));
  });

  it('looks in the Windows font directories, including the per-user one', () => {
    const dirs = candidates('win32', {
      USERPROFILE: 'C:\\Users\\u',
      WINDIR: 'C:\\Windows',
      LOCALAPPDATA: 'C:\\Users\\u\\AppData\\Local',
    });
    expect(dirs).toContain(path.join('C:\\Windows', 'Fonts'));
    expect(dirs.some((dir) => dir.includes('Microsoft'))).toBe(true);
  });

  it('offers nothing that is not a directory', async () => {
    // The real function, on the real host: everything it returns must exist.
    const fs = await import('fs');
    for (const dir of systemFontDirs()) {
      expect(fs.statSync(dir).isDirectory()).toBe(true);
    }
  });

  it('never returns an empty candidate list for any platform', () => {
    for (const platform of ['darwin', 'win32', 'linux', 'freebsd'] as NodeJS.Platform[]) {
      expect(candidates(platform, { HOME: '/h' }).length).toBeGreaterThan(0);
    }
  });
});
