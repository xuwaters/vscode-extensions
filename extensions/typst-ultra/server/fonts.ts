import * as fs from 'fs';
import * as path from 'path';

/** One face, as the Rust side wants it. */
export interface FaceEntry {
  /** Upstream `FontInfo`, serialized. Opaque here. */
  info: unknown;
  /** The face's index within its container file. */
  index: number;
  /** The file it came from. Host-side only; not sent across. */
  file: string;
}

/** The on-disk index for one font file. */
interface CachedFile {
  /** Modification time in milliseconds, for invalidation. */
  mtimeMs: number;
  /** File size in bytes, for invalidation. */
  size: number;
  /** The faces the file contains. */
  faces: { info: unknown; index: number }[];
}

/** The whole cache, keyed by absolute path. */
type Cache = Record<string, CachedFile>;

const FONT_EXTENSIONS = new Set(['.ttf', '.otf', '.ttc', '.otc']);

/**
 * Font discovery and the on-disk index cache.
 *
 * Fonts load in two stages. The bundled set is indexed first — it is small,
 * it is what the overwhelming majority of documents use, and having it means
 * the first compile is already correct. System fonts follow in the background,
 * and produce one recompile when they land.
 *
 * The expensive part is parsing metadata out of every font file, so the result
 * is cached on disk keyed by `path + mtime + size`. A designer with 400 MB of
 * installed fonts pays that once per machine rather than once per session.
 */
export class FontIndex {
  private cache: Cache = {};
  private faces: FaceEntry[] = [];

  constructor(
    private readonly cachePath: string,
    /** Parses one font file's metadata. Backed by `TypstServer.indexFont`. */
    private readonly parse: (data: Uint8Array) => { info: unknown; index: number }[],
  ) {
    this.cache = readCache(cachePath);
  }

  /** Every face indexed so far. */
  get entries(): FaceEntry[] {
    return this.faces;
  }

  /** What crosses the WASM boundary: metadata only, no bytes. */
  get descriptors(): { info: unknown; index: number }[] {
    return this.faces.map((face) => ({ info: face.info, index: face.index }));
  }

  /** The bytes for one face, read on demand. */
  data(face: number): Uint8Array | null {
    const entry = this.faces[face];
    if (!entry) return null;
    try {
      return new Uint8Array(fs.readFileSync(entry.file));
    } catch {
      return null;
    }
  }

  /**
   * Index every font file under the given directories.
   *
   * Returns how many milliseconds it took and how many files had to be parsed
   * rather than served from the cache — the numbers P1-11 and P2-13 exist to
   * put on the record.
   */
  addDirectories(dirs: string[]): { ms: number; parsed: number; faces: number } {
    const started = Date.now();
    let parsed = 0;

    for (const dir of dirs) {
      for (const file of walk(dir)) {
        if (this.addFile(file)) parsed += 1;
      }
    }

    return {
      ms: Date.now() - started,
      parsed,
      faces: this.faces.length,
    };
  }

  /** Index one file. Returns whether it had to be parsed rather than cached. */
  addFile(file: string): boolean {
    let stat: fs.Stats;
    try {
      stat = fs.statSync(file);
    } catch {
      return false;
    }

    const cached = this.cache[file];
    if (cached && cached.mtimeMs === stat.mtimeMs && cached.size === stat.size) {
      this.push(file, cached.faces);
      return false;
    }

    let faces: { info: unknown; index: number }[];
    try {
      faces = this.parse(new Uint8Array(fs.readFileSync(file)));
    } catch {
      return false;
    }

    this.cache[file] = { mtimeMs: stat.mtimeMs, size: stat.size, faces };
    this.push(file, faces);
    return true;
  }

  /** Write the index back to disk. */
  save(): void {
    if (!this.cachePath) return;
    try {
      fs.mkdirSync(path.dirname(this.cachePath), { recursive: true });
      fs.writeFileSync(this.cachePath, JSON.stringify(this.cache));
    } catch {
      // A cache that cannot be written costs a rescan next time, nothing more.
    }
  }

  private push(file: string, faces: { info: unknown; index: number }[]): void {
    for (const face of faces) {
      this.faces.push({ info: face.info, index: face.index, file });
    }
  }
}

/** Read the cache, tolerating a missing or corrupt file. */
function readCache(cachePath: string): Cache {
  if (!cachePath) return {};
  try {
    const parsed: unknown = JSON.parse(fs.readFileSync(cachePath, 'utf8'));
    return isCache(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

function isCache(value: unknown): value is Cache {
  if (typeof value !== 'object' || value === null) return false;
  return Object.values(value).every(
    (entry) =>
      typeof entry === 'object' &&
      entry !== null &&
      typeof (entry as CachedFile).mtimeMs === 'number' &&
      typeof (entry as CachedFile).size === 'number' &&
      Array.isArray((entry as CachedFile).faces),
  );
}

/** Every font file under a directory, recursively, in a stable order. */
export function* walk(dir: string): Generator<string> {
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }

  // Sorted so an index built twice lists faces in the same order, which keeps
  // font *selection* stable between sessions.
  entries.sort((a, b) => a.name.localeCompare(b.name));

  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      yield* walk(full);
    } else if (FONT_EXTENSIONS.has(path.extname(entry.name).toLowerCase())) {
      yield full;
    }
  }
}

/**
 * Where a platform keeps installed fonts, before checking whether the
 * directories are there.
 *
 * Split from [`systemFontDirs`] so the per-platform lists can be tested from any
 * host — a Windows-only path bug should be a failing test, not a bug report from
 * the one person who runs Windows.
 */
export function systemFontCandidates(
  platform: NodeJS.Platform = process.platform,
  env: NodeJS.ProcessEnv = process.env,
): string[] {
  const home = env.HOME ?? env.USERPROFILE ?? '';

  if (platform === 'darwin') {
    return [
      '/System/Library/Fonts',
      '/Library/Fonts',
      path.join(home, 'Library/Fonts'),
    ];
  }

  if (platform === 'win32') {
    return [
      path.join(env.WINDIR ?? 'C:\\Windows', 'Fonts'),
      path.join(
        env.LOCALAPPDATA ?? path.join(home, 'AppData', 'Local'),
        'Microsoft',
        'Windows',
        'Fonts',
      ),
    ];
  }

  return [
    '/usr/share/fonts',
    '/usr/local/share/fonts',
    path.join(home, '.fonts'),
    path.join(home, '.local/share/fonts'),
  ];
}

/**
 * Where this machine keeps installed fonts.
 *
 * Only directories that exist are returned, so a missing one costs nothing.
 */
export function systemFontDirs(): string[] {
  return systemFontCandidates().filter((dir) => {
    try {
      return fs.statSync(dir).isDirectory();
    } catch {
      return false;
    }
  });
}
