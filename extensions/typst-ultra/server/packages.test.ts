import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, describe, expect, it } from 'vitest';
import { defaultCacheDir, extractTar, readTar, readTemplate } from './packages.js';

/** Build a tar archive in memory, so the traversal test needs no fixture file. */
function tar(entries: { name: string; body?: string; type?: string }[]): Buffer {
  const blocks: Buffer[] = [];

  for (const entry of entries) {
    const body = Buffer.from(entry.body ?? '');
    const header = Buffer.alloc(512);

    header.write(entry.name, 0, 100, 'utf8');
    header.write('0000644\0', 100, 8, 'utf8');
    header.write('0000000\0', 108, 8, 'utf8');
    header.write('0000000\0', 116, 8, 'utf8');
    header.write(body.length.toString(8).padStart(11, '0') + '\0', 124, 12, 'utf8');
    header.write('00000000000\0', 136, 12, 'utf8');
    header.write(entry.type ?? '0', 156, 1, 'utf8');
    header.write('ustar\0' + '00', 257, 8, 'utf8');

    // The checksum is computed with the field itself read as spaces.
    header.write(' '.repeat(8), 148, 8, 'utf8');
    let checksum = 0;
    for (const byte of header) checksum += byte;
    header.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'utf8');

    blocks.push(header);
    const padded = Buffer.alloc(Math.ceil(body.length / 512) * 512);
    body.copy(padded);
    blocks.push(padded);
  }

  blocks.push(Buffer.alloc(1024));
  return Buffer.concat(blocks);
}

describe('package archives', () => {
  const dirs: string[] = [];

  const scratch = () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'typst-pkg-'));
    dirs.push(dir);
    return dir;
  };

  afterEach(() => {
    for (const dir of dirs.splice(0)) {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it('reads entries out of an archive', () => {
    const entries = readTar(
      tar([
        { name: 'typst.toml', body: '[package]\nname = "demo"\n' },
        { name: 'src/lib.typ', body: '#let x = 1\n' },
      ]),
    );

    expect(entries.map((entry) => entry.name)).toEqual(['typst.toml', 'src/lib.typ']);
    expect(entries[1].data.toString()).toBe('#let x = 1\n');
  });

  it('extracts nested files', () => {
    const into = scratch();
    extractTar(
      tar([
        { name: 'typst.toml', body: 'name\n' },
        { name: 'src/lib.typ', body: 'body\n' },
      ]),
      into,
    );

    expect(fs.readFileSync(path.join(into, 'typst.toml'), 'utf8')).toBe('name\n');
    expect(fs.readFileSync(path.join(into, 'src/lib.typ'), 'utf8')).toBe('body\n');
  });

  /**
   * The case this check exists for. A package comes from the network, and an
   * archive naming `../../evil` would otherwise write outside the cache.
   */
  it('refuses to extract outside its directory', () => {
    const into = scratch();
    const escape = path.join(into, '..', 'escaped.txt');

    expect(() =>
      extractTar(tar([{ name: '../escaped.txt', body: 'pwned' }]), into),
    ).toThrow(/refusing to extract outside/);

    expect(fs.existsSync(escape)).toBe(false);
  });

  it('refuses an absolute path in an archive', () => {
    const into = scratch();
    expect(() =>
      extractTar(tar([{ name: '/tmp/typst-escape.txt', body: 'pwned' }]), into),
    ).toThrow(/refusing to extract outside/);
  });

  it('skips long-name and extended-header entries rather than writing them out', () => {
    const into = scratch();
    extractTar(
      tar([
        { name: '././@LongLink', body: 'some/very/long/name', type: 'L' },
        { name: 'typst.toml', body: 'ok\n' },
      ]),
      into,
    );

    expect(fs.existsSync(path.join(into, '@LongLink'))).toBe(false);
    expect(fs.readFileSync(path.join(into, 'typst.toml'), 'utf8')).toBe('ok\n');
  });

  it('picks a platform-appropriate cache directory', () => {
    const dir = defaultCacheDir();
    expect(dir).toMatch(/typst[/\\]packages$/);
    expect(path.isAbsolute(dir)).toBe(true);
  });
});

describe('the template manifest reader', () => {
  it('reads a template section', () => {
    const manifest = `
[package]
name = "charged-ieee"
version = "0.1.4"
entrypoint = "lib.typ"

[template]
path = "template"
entrypoint = "main.typ"
thumbnail = "thumbnail.png"
`;
    expect(readTemplate(manifest)).toEqual({
      path: 'template',
      entrypoint: 'main.typ',
      thumbnail: 'thumbnail.png',
    });
  });

  it('accepts a template without a thumbnail', () => {
    const manifest = '[template]\npath = "t"\nentrypoint = "main.typ"\n';
    expect(readTemplate(manifest)).toEqual({ path: 't', entrypoint: 'main.typ' });
  });

  it('returns nothing for a package that is not a template', () => {
    const manifest = '[package]\nname = "cetz"\nversion = "0.4.2"\n';
    expect(readTemplate(manifest)).toBeNull();
  });

  it('returns nothing when the section is incomplete', () => {
    expect(readTemplate('[template]\npath = "t"\n')).toBeNull();
    expect(readTemplate('[template]\nentrypoint = "main.typ"\n')).toBeNull();
  });

  it('does not read keys from a later section', () => {
    const manifest = `
[template]
path = "t"

[tool.other]
entrypoint = "not-mine.typ"
`;
    expect(readTemplate(manifest), 'entrypoint belongs to [tool.other]').toBeNull();
  });

  it('accepts single quotes and stray whitespace', () => {
    const manifest = "[template]\n  path   =   'tpl'  \n  entrypoint='main.typ'\n";
    expect(readTemplate(manifest)).toEqual({ path: 'tpl', entrypoint: 'main.typ' });
  });

  it('handles CRLF line endings', () => {
    const manifest = '[template]\r\npath = "t"\r\nentrypoint = "main.typ"\r\n';
    expect(readTemplate(manifest)).toEqual({ path: 't', entrypoint: 'main.typ' });
  });
});
