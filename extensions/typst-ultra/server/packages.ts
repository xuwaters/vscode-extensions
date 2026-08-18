import * as fs from 'fs';
import * as path from 'path';
import * as zlib from 'zlib';
import { baseOf, isInside, type Roots } from './vfs.js';

/** What the Rust side is told about a package, verbatim. */
export type Resolution = 'ready' | 'pending' | `failed:${string}`;

/** How the download of one package is going. */
type State =
  | { kind: 'downloading' }
  | { kind: 'ready' }
  | { kind: 'failed'; reason: string };

export interface PackagesOptions {
  /** `typstUltra.packages.enabled`. */
  enabled: boolean;
  /** `typstUltra.packages.registry`. */
  registry: string;
  /** Where downloads land. Shared with typst-cli so nothing is fetched twice. */
  cache: string;
  /** Called when a package becomes available, so the host can recompile. */
  onReady: (spec: string) => void;
  /** Called on any state change, for `typst/packageStatus`. */
  onStatus: (spec: string, state: State) => void;
}

/**
 * Package resolution and download.
 *
 * The Rust side asks synchronously, mid-compile, and cannot wait: WASM has no
 * network and the compile is in flight. So a package that is not on disk yet
 * comes back `pending`, the compile finishes with a diagnostic naming it, the
 * download runs here, and the host triggers a recompile when it lands.
 */
export class Packages {
  private readonly states = new Map<string, State>();

  constructor(private options: PackagesOptions) {}

  /** Replace the options, e.g. after a settings change. */
  configure(options: Partial<PackagesOptions>): void {
    this.options = { ...this.options, ...options };
  }

  /** The cache directory in use. */
  get cache(): string {
    return this.options.cache;
  }

  /** Answer the Rust side's synchronous question. */
  resolve(spec: string, roots: Roots): Resolution {
    const dir = baseOf(roots, spec);
    if (!dir) return 'failed:the package cache is not configured';

    if (fs.existsSync(path.join(dir, 'typst.toml'))) {
      this.states.set(spec, { kind: 'ready' });
      return 'ready';
    }

    if (!this.options.enabled) {
      return 'failed:package downloads are disabled (typstUltra.packages.enabled)';
    }

    const state = this.states.get(spec);
    if (state?.kind === 'failed') return `failed:${state.reason}`;
    if (state?.kind === 'downloading') return 'pending';

    this.states.set(spec, { kind: 'downloading' });
    this.options.onStatus(spec, { kind: 'downloading' });
    void this.download(spec, dir);
    return 'pending';
  }

  /** Forget every download outcome, so failures are retried. */
  reset(): void {
    this.states.clear();
  }

  /**
   * Download a package if needed and wait for it — the template flow's entry
   * point (P4-14).
   *
   * The compile path deliberately cannot do this: it is synchronous and
   * mid-flight. Scaffolding a project is not, so it can simply wait.
   */
  async ensure(spec: string, roots: Roots): Promise<string | null> {
    const dir = baseOf(roots, spec);
    if (!dir) return null;

    if (fs.existsSync(path.join(dir, 'typst.toml'))) return dir;
    if (!this.options.enabled) return null;

    const parsed = /^@([^/]+)\/([^:]+):(.+)$/.exec(spec);
    if (!parsed) return null;

    await this.download(spec, dir);
    return this.states.get(spec)?.kind === 'ready' ? dir : null;
  }

  /** Delete the cache directory. */
  clearCache(): void {
    if (!this.options.cache) return;
    fs.rmSync(this.options.cache, { recursive: true, force: true });
    this.reset();
  }

  private async download(spec: string, dir: string): Promise<void> {
    const parsed = /^@([^/]+)\/([^:]+):(.+)$/.exec(spec);
    if (!parsed) {
      this.fail(spec, 'malformed package spec');
      return;
    }
    const [, namespace, name, version] = parsed;

    const url = `${this.options.registry.replace(/\/$/, '')}/${namespace}/${name}-${version}.tar.gz`;
    try {
      const response = await fetch(url);
      if (!response.ok) {
        this.fail(spec, `${response.status} ${response.statusText} from ${url}`);
        return;
      }

      const archive = Buffer.from(await response.arrayBuffer());
      const tar = zlib.gunzipSync(archive);
      extractTar(tar, dir);

      this.states.set(spec, { kind: 'ready' });
      this.options.onStatus(spec, { kind: 'ready' });
      this.options.onReady(spec);
    } catch (error) {
      this.fail(spec, error instanceof Error ? error.message : String(error));
    }
  }

  private fail(spec: string, reason: string): void {
    this.states.set(spec, { kind: 'failed', reason });
    this.options.onStatus(spec, { kind: 'failed', reason });
    // A failed package still needs a recompile so the diagnostic on the import
    // line changes from "downloading" to the real reason.
    this.options.onReady(spec);
  }
}

/** One entry of a tar archive. */
interface TarEntry {
  name: string;
  data: Buffer;
  isFile: boolean;
}

/**
 * Extract a tar archive into a directory.
 *
 * Written by hand rather than pulled in as a dependency: the parsing is fifty
 * lines and the path-traversal check has to be ours anyway, since a malicious
 * archive naming `../../.ssh/authorized_keys` is exactly the case this has to
 * refuse.
 */
export function extractTar(tar: Buffer, into: string): void {
  fs.mkdirSync(into, { recursive: true });

  for (const entry of readTar(tar)) {
    const target = path.resolve(into, entry.name);
    if (!isInside(into, target)) {
      throw new Error(`refusing to extract outside the package directory: ${entry.name}`);
    }

    if (!entry.isFile) {
      fs.mkdirSync(target, { recursive: true });
      continue;
    }

    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, entry.data);
  }
}

/** Iterate a tar archive's entries. */
export function readTar(tar: Buffer): TarEntry[] {
  const entries: TarEntry[] = [];
  let offset = 0;

  while (offset + 512 <= tar.length) {
    const header = tar.subarray(offset, offset + 512);
    // Two consecutive zero blocks end the archive.
    if (header.every((byte) => byte === 0)) break;

    const name = cstring(header.subarray(0, 100));
    const size = parseInt(cstring(header.subarray(124, 136)).trim() || '0', 8);
    const typeflag = String.fromCharCode(header[156]);
    // GNU/PAX long-name and extended-header entries carry no file content we
    // want; skip their payload rather than writing it out as a file.
    const skip = typeflag === 'L' || typeflag === 'K' || typeflag === 'x' || typeflag === 'g';
    const prefix = cstring(header.subarray(345, 500));

    offset += 512;
    const data = tar.subarray(offset, offset + size);
    offset += Math.ceil(size / 512) * 512;

    if (!name || skip) continue;

    entries.push({
      name: prefix ? `${prefix}/${name}` : name,
      data: Buffer.from(data),
      isFile: typeflag === '0' || typeflag === '\0' || typeflag === '',
    });
  }

  return entries;
}

/** The `[template]` section of a package manifest, if it has one. */
export interface TemplateInfo {
  /** The directory inside the package to copy. */
  path: string;
  /** The file to open afterwards, relative to `path`. */
  entrypoint: string;
  /** A thumbnail, relative to the package root. */
  thumbnail?: string;
}

/**
 * Read the `[template]` section out of a `typst.toml`.
 *
 * A focused reader rather than a TOML dependency: the section is three string
 * keys, and the alternative is a parser in the VSIX for one shape of one file.
 * Same trade as the tar reader above.
 */
export function readTemplate(manifest: string): TemplateInfo | null {
  const section = sectionOf(manifest, 'template');
  if (!section) return null;

  const path = stringKey(section, 'path');
  const entrypoint = stringKey(section, 'entrypoint');
  if (!path || !entrypoint) return null;

  const thumbnail = stringKey(section, 'thumbnail');
  return thumbnail ? { path, entrypoint, thumbnail } : { path, entrypoint };
}

/** The lines of one `[section]`, up to the next section header. */
function sectionOf(manifest: string, name: string): string | null {
  const lines = manifest.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === `[${name}]`);
  if (start === -1) return null;

  const rest = lines.slice(start + 1);
  const end = rest.findIndex((line) => /^\s*\[/.test(line));
  return (end === -1 ? rest : rest.slice(0, end)).join('\n');
}

/** A `key = "value"` pair, with either quote style. */
function stringKey(section: string, key: string): string | undefined {
  const match = new RegExp(`^\\s*${key}\\s*=\\s*("[^"]*"|'[^']*')`, 'm').exec(section);
  return match ? match[1].slice(1, -1) : undefined;
}

function cstring(buffer: Buffer): string {
  const end = buffer.indexOf(0);
  return buffer.subarray(0, end === -1 ? buffer.length : end).toString('utf8');
}

/**
 * typst's standard package cache directory, so downloads are shared with
 * `typst-cli` and nothing is fetched twice.
 */
export function defaultCacheDir(): string {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? '';
  if (process.platform === 'win32') {
    const local = process.env.LOCALAPPDATA ?? path.join(home, 'AppData', 'Local');
    return path.join(local, 'typst', 'packages');
  }
  if (process.platform === 'darwin') {
    return path.join(home, 'Library', 'Caches', 'typst', 'packages');
  }
  const xdg = process.env.XDG_CACHE_HOME ?? path.join(home, '.cache');
  return path.join(xdg, 'typst', 'packages');
}
