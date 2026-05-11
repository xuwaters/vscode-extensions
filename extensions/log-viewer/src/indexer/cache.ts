// Persistent index cache.
//
// Format mirrors `crates/log-engine/src/index.rs` byte-for-byte:
//
//   magic       "LGIX"     4 B
//   version     u32        4 B  (currently 1)
//   stride      u32        4 B
//   total_lines u64        8 B
//   file_size   u64        8 B
//   mtime_ms    u64        8 B
//   path_hash   [16]B     16 B  (sha1(absoluteFsPath) prefix)
//   anchor_count u64       8 B
//   anchors[]: u64         anchor_count * 8 B
//
// All multi-byte fields are little-endian. Keeping the format identical to
// the engine crate lets a future out-of-process server load indexes the
// VSCode extension wrote, and vice versa (RFC §11.5).

import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';

export const INDEX_MAGIC = Buffer.from('LGIX', 'ascii');
export const INDEX_VERSION = 1;
export const INDEX_HEADER_SIZE = 4 + 4 + 4 + 8 + 8 + 8 + 16 + 8;
export const PATH_HASH_BYTES = 16;

export interface IndexHeader {
  stride: number;
  totalLines: number;
  fileSize: number;
  mtimeMs: number;
  /** First 16 bytes of sha1(absoluteFsPath). */
  pathHash: Buffer;
  anchorCount: number;
}

export interface IndexFile {
  header: IndexHeader;
  anchors: BigUint64Array;
}

export type IndexLocationMode = 'globalStorage' | 'adjacent' | 'directory';

export interface CachePaths {
  /**
   * The extension's global storage directory (`context.globalStorageUri.fsPath`).
   * The resolver writes index files to `<globalStorageDir>/index/` so the
   * directory we manage doesn't collide with other extension state.
   */
  globalStorageDir: string;
  /** Optional user-configured directory; resolved with `~` expansion. */
  customDir?: string;
}

/** Concrete directory the centralised cache lives in for `globalStorage` mode. */
export function globalIndexDir(globalStorageDir: string): string {
  return path.join(globalStorageDir, 'index');
}

/** Stable cache filename for a given log file. */
export function cacheKey(fsPath: string, size: number, mtimeMs: number): string {
  const hash = crypto.createHash('sha1').update(fsPath).digest('hex').slice(0, 16);
  return `${hash}-${size}-${mtimeMs}.idx`;
}

export function pathHashOf(fsPath: string): Buffer {
  return crypto.createHash('sha1').update(fsPath).digest().subarray(0, PATH_HASH_BYTES);
}

export function resolveCachePath(
  mode: IndexLocationMode,
  logPath: string,
  size: number,
  mtimeMs: number,
  paths: CachePaths,
): string {
  switch (mode) {
    case 'adjacent': {
      const dir = path.dirname(logPath);
      const base = path.basename(logPath);
      return path.join(dir, `${base}.bin`);
    }
    case 'directory': {
      const customDir = paths.customDir;
      if (!customDir) {
        throw new Error('indexLocation=directory requires customDir');
      }
      return path.join(expandHome(customDir), cacheKey(logPath, size, mtimeMs));
    }
    case 'globalStorage':
    default:
      return path.join(
        globalIndexDir(paths.globalStorageDir),
        cacheKey(logPath, size, mtimeMs),
      );
  }
}

function expandHome(p: string): string {
  if (p.startsWith('~/') || p === '~') {
    const home = process.env['HOME'] ?? process.env['USERPROFILE'];
    if (home) return path.join(home, p.slice(1).replace(/^[/\\]/, ''));
  }
  return p;
}

export function encodeIndexFile(file: IndexFile): Buffer {
  const { header, anchors } = file;
  if (header.pathHash.length < PATH_HASH_BYTES) {
    throw new Error('pathHash must be at least 16 bytes');
  }
  const out = Buffer.alloc(INDEX_HEADER_SIZE + anchors.length * 8);
  let off = 0;
  INDEX_MAGIC.copy(out, off);
  off += 4;
  out.writeUInt32LE(INDEX_VERSION, off);
  off += 4;
  out.writeUInt32LE(header.stride, off);
  off += 4;
  out.writeBigUInt64LE(BigInt(header.totalLines), off);
  off += 8;
  out.writeBigUInt64LE(BigInt(header.fileSize), off);
  off += 8;
  out.writeBigUInt64LE(BigInt(header.mtimeMs), off);
  off += 8;
  header.pathHash.copy(out, off, 0, PATH_HASH_BYTES);
  off += PATH_HASH_BYTES;
  out.writeBigUInt64LE(BigInt(anchors.length), off);
  off += 8;
  for (let i = 0; i < anchors.length; i++) {
    out.writeBigUInt64LE(anchors[i], off);
    off += 8;
  }
  return out;
}

export class DecodeIndexError extends Error {
  constructor(
    message: string,
    public readonly reason:
      | 'truncated'
      | 'bad-magic'
      | 'unsupported-version'
      | 'size-mismatch'
      | 'mtime-mismatch'
      | 'path-mismatch',
  ) {
    super(message);
  }
}

export interface DecodeOptions {
  expectedSize?: number;
  expectedMtimeMs?: number;
  expectedPathHash?: Buffer;
}

/** Decode an .idx buffer. Rejects on header mismatch with caller's expectations. */
export function decodeIndexFile(buf: Buffer, opts: DecodeOptions = {}): IndexFile {
  if (buf.length < INDEX_HEADER_SIZE) {
    throw new DecodeIndexError('index file truncated (header)', 'truncated');
  }
  if (!buf.subarray(0, 4).equals(INDEX_MAGIC)) {
    throw new DecodeIndexError('index file has bad magic', 'bad-magic');
  }
  const version = buf.readUInt32LE(4);
  if (version !== INDEX_VERSION) {
    throw new DecodeIndexError(
      `unsupported index version: ${version}`,
      'unsupported-version',
    );
  }
  const stride = buf.readUInt32LE(8);
  const totalLines = Number(buf.readBigUInt64LE(12));
  const fileSize = Number(buf.readBigUInt64LE(20));
  const mtimeMs = Number(buf.readBigUInt64LE(28));
  const pathHash = Buffer.from(buf.subarray(36, 36 + PATH_HASH_BYTES));
  const anchorCount = Number(buf.readBigUInt64LE(52));

  if (opts.expectedSize !== undefined && opts.expectedSize !== fileSize) {
    throw new DecodeIndexError(
      `file size drifted (cache=${fileSize}, current=${opts.expectedSize})`,
      'size-mismatch',
    );
  }
  if (opts.expectedMtimeMs !== undefined && opts.expectedMtimeMs !== mtimeMs) {
    throw new DecodeIndexError(
      `mtime drifted (cache=${mtimeMs}, current=${opts.expectedMtimeMs})`,
      'mtime-mismatch',
    );
  }
  if (opts.expectedPathHash && !opts.expectedPathHash.equals(pathHash)) {
    throw new DecodeIndexError('path hash mismatch', 'path-mismatch');
  }

  const expectedBytes = INDEX_HEADER_SIZE + anchorCount * 8;
  if (buf.length < expectedBytes) {
    throw new DecodeIndexError(
      `index file truncated (have ${buf.length}, want ${expectedBytes})`,
      'truncated',
    );
  }
  const anchors = new BigUint64Array(anchorCount);
  for (let i = 0; i < anchorCount; i++) {
    anchors[i] = buf.readBigUInt64LE(INDEX_HEADER_SIZE + i * 8);
  }
  return {
    header: { stride, totalLines, fileSize, mtimeMs, pathHash, anchorCount },
    anchors,
  };
}

/** Atomically write an .idx file. */
export function writeIndexFile(filePath: string, file: IndexFile): void {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  const tmp = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  const bytes = encodeIndexFile(file);
  fs.writeFileSync(tmp, bytes);
  fs.renameSync(tmp, filePath);
}

export function tryReadIndexFile(
  filePath: string,
  opts: DecodeOptions = {},
): IndexFile | null {
  let buf: Buffer;
  try {
    buf = fs.readFileSync(filePath);
  } catch {
    return null;
  }
  try {
    return decodeIndexFile(buf, opts);
  } catch {
    return null;
  }
}

export interface EvictionResult {
  scannedBytes: number;
  removedBytes: number;
  removedFiles: number;
}

/** Drop oldest-`atime` `.idx` files under `dir` until total size ≤ budgetBytes. */
export function evictCache(dir: string, budgetBytes: number): EvictionResult {
  let scannedBytes = 0;
  let removedBytes = 0;
  let removedFiles = 0;
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return { scannedBytes, removedBytes, removedFiles };
  }
  const files: Array<{ name: string; size: number; atime: number }> = [];
  for (const ent of entries) {
    if (!ent.isFile() || !ent.name.endsWith('.idx')) continue;
    const fp = path.join(dir, ent.name);
    try {
      const st = fs.statSync(fp);
      files.push({ name: fp, size: st.size, atime: st.atimeMs });
      scannedBytes += st.size;
    } catch {
      // ignore
    }
  }
  if (scannedBytes <= budgetBytes) {
    return { scannedBytes, removedBytes, removedFiles };
  }
  files.sort((a, b) => a.atime - b.atime); // oldest first
  let remaining = scannedBytes;
  for (const f of files) {
    if (remaining <= budgetBytes) break;
    try {
      fs.unlinkSync(f.name);
      remaining -= f.size;
      removedBytes += f.size;
      removedFiles += 1;
    } catch {
      // ignore
    }
  }
  return { scannedBytes, removedBytes, removedFiles };
}
